use ::bytes::Bytes;
use base64::{engine::general_purpose::STANDARD, Engine};
use md5::{Digest, Md5};
use regex::Regex;
use reqwest::{
    cookie::{self, CookieStore, Jar},
    header::{
        HeaderValue, ACCEPT, ACCEPT_ENCODING, ACCEPT_LANGUAGE, CONTENT_RANGE, CONTENT_TYPE, RANGE,
        REFERER,
    },
    multipart, Body, Response, StatusCode,
};
use select::{document::Document, node::Node, predicate::Name};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::json;
use std::{
    cmp::min,
    collections::HashMap,
    fs,
    io::Write,
    ops::Deref,
    path::Path,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::Url;

use crate::{
    error::{ClientError, Result},
    model::{
        Assignment, CalendarEvent, Colors, ConfirmChunkUploadResult, Course, File, Folder,
        ItemPage, JBoxErrorMessage, JBoxLoginInfo, JboxLoginResult, PersonalSpaceInfo,
        ProgressPayload, StartChunkUploadContext, Subject, Submission, SubmissionUploadResult,
        SubmissionUploadSuccessResponse, User, VideoCourse, VideoInfo, VideoPlayInfo,
    },
};
const BASE_URL: &str = "https://oc.sjtu.edu.cn";
const VIDEO_BASE_URL: &str = "https://courses.sjtu.edu.cn/app";
const VIDEO_LOGIN_URL: &str = "https://courses.sjtu.edu.cn/app/oauth/2.0/login?login_type=outer";
const VIDEO_OAUTH_KEY_URL: &str = "https://courses.sjtu.edu.cn/app/vodvideo/vodVideoPlay.d2j?ssoCheckToken=ssoCheckToken&refreshToken=&accessToken=&userId=&";
const VIDEO_INFO_URL: &str =
    "https://courses.sjtu.edu.cn/app/system/resource/vodVideo/getvideoinfos";
const AUTH_URL: &str = "https://jaccount.sjtu.edu.cn";
const MY_SJTU_URL: &str = "https://my.sjtu.edu.cn/ui/appmyinfo";
const EXPRESS_LOGIN_URL: &str = "https://jaccount.sjtu.edu.cn/jaccount/expresslogin";
const OAUTH_PATH: &str =
    "aHR0cHM6Ly9jb3Vyc2VzLnNqdHUuZWR1LmNuL2FwcC92b2R2aWRlby92b2RWaWRlb1BsYXkuZDJq";
const OAUTH_RANDOM: &str = "oauth_ABCDE=ABCDEFGH&oauth_VWXYZ=STUVWXYZ";
const OAUTH_RANDOM_P1: &str = "oauth_ABCDE";
const OAUTH_RANDOM_P2: &str = "oauth_VWXYZ";
const OAUTH_RANDOM_P1_VAL: &str = "ABCDEFGH";
const OAUTH_RANDOM_P2_VAL: &str = "STUVWXYZ";
const CHUNK_SIZE: u64 = 512 * 1024;
const VIDEO_CHUNK_SIZE: u64 = 4 * 1024 * 1024;

const JBOX_LOGIN_URL: &str = "https://pan.sjtu.edu.cn/user/v1/sign-in/sso-login-redirect/xpw8ou8y";
const JBOX_LOGIN_URL2: &str = "https://pan.sjtu.edu.cn/user/v1/sign-in/verify-account-login/xpw8ou8y?device_id=Chrome+116.0.0.0&type=sso&credential=";
const JBOX_USER_SPACE_URL: &str = "https://pan.sjtu.edu.cn/user/v1/space/1/personal";
const JBOX_BASE_URL: &str = "https://pan.sjtu.edu.cn";
// 4M
const JBOX_UPLOAD_CHUNK_SIZE: usize = 4 * 1024 * 1024;
pub struct Client {
    cli: reqwest::Client,
    jar: Arc<Jar>,
}

// Apis here are for canvas
impl Client {
    pub fn new() -> Self {
        let jar = Arc::new(cookie::Jar::default());
        let cli = reqwest::Client::builder()
            .cookie_provider(jar.clone())
            .build()
            .unwrap();
        Self { cli, jar }
    }

    async fn get_request_with_token<T: Serialize + ?Sized>(
        &self,
        url: &str,
        query: Option<&T>,
        token: &str,
    ) -> Result<Response> {
        let mut req = self
            .cli
            .get(url)
            .header("Authorization", format!("Bearer {}", token));

        if let Some(query) = query {
            req = req.query(query)
        }

        let res = req.send().await?;
        Ok(res)
    }

    async fn get_json_with_token<T: Serialize + ?Sized, D: DeserializeOwned>(
        &self,
        url: &str,
        query: Option<&T>,
        token: &str,
    ) -> Result<D> {
        let response = self
            .get_request_with_token(url, query, token)
            .await?
            .error_for_status()?;
        let json = serde_json::from_slice(&response.bytes().await?)?;
        Ok(json)
    }

    pub async fn post_form_with_token<T: Serialize + ?Sized, Q: Serialize + ?Sized>(
        &self,
        url: &str,
        query: Option<&Q>,
        form: &T,
        token: &str,
    ) -> Result<Response> {
        let mut request = self
            .cli
            .post(url)
            .header("Authorization".to_owned(), format!("Bearer {}", token))
            .form(form);
        if let Some(query) = query {
            request = request.query(query);
        }
        let response = request.send().await?;
        Ok(response)
    }

    pub async fn put_form_with_token<T: Serialize + ?Sized, Q: Serialize + ?Sized>(
        &self,
        url: &str,
        query: Option<&Q>,
        form: &T,
        token: &str,
    ) -> Result<Response> {
        let mut request = self
            .cli
            .put(url)
            .header("Authorization".to_owned(), format!("Bearer {}", token))
            .form(form);
        if let Some(query) = query {
            request = request.query(query);
        }
        let response = request.send().await?;
        Ok(response)
    }

    pub async fn delete_submission_comment(
        &self,
        course_id: i64,
        assignment_id: i64,
        student_id: &str,
        comment_id: i64,
        token: &str,
    ) -> Result<()> {
        let url = format!(
            "{}/api/v1/courses/{}/assignments/{}/submissions/{}/comments/{}",
            BASE_URL, course_id, assignment_id, student_id, comment_id
        );
        self.cli
            .delete(url)
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    pub async fn update_grade(
        &self,
        course_id: i64,
        assignment_id: i64,
        student_id: i64,
        grade: &str,
        comment: Option<&str>,
        token: &str,
    ) -> Result<()> {
        let url = format!(
            "{}/api/v1/courses/{}/assignments/{}/submissions/update_grades",
            BASE_URL, course_id, assignment_id
        );
        let form = match comment {
            Some(comment) => vec![
                (format!("grade_data[{}][posted_grade]", student_id), grade),
                (format!("grade_data[{}][text_comment]", student_id), comment),
            ],
            None => vec![(format!("grade_data[{}][posted_grade]", student_id), grade)],
        };
        self.post_form_with_token(&url, None::<&str>, &form, token)
            .await?
            .error_for_status()?;
        Ok(())
    }

    pub async fn modify_assignment_ddl(
        &self,
        course_id: i64,
        assignment_id: i64,
        due_at: Option<&str>,
        lock_at: Option<&str>,
        token: &str,
    ) -> Result<()> {
        let url = format!(
            "{}/api/v1/courses/{}/assignments/{}",
            BASE_URL, course_id, assignment_id
        );
        self.put_form_with_token(
            &url,
            None::<&str>,
            &[
                ("assignment[due_at]", due_at.unwrap_or_default()),
                ("assignment[lock_at]", lock_at.unwrap_or_default()),
            ],
            token,
        )
        .await?
        .error_for_status()?;
        Ok(())
    }

    pub async fn modify_assignment_ddl_override(
        &self,
        course_id: i64,
        assignment_id: i64,
        override_id: i64,
        due_at: Option<&str>,
        lock_at: Option<&str>,
        token: &str,
    ) -> Result<()> {
        let url = format!(
            "{}/api/v1/courses/{}/assignments/{}/overrides/{}",
            BASE_URL, course_id, assignment_id, override_id
        );
        self.put_form_with_token(
            &url,
            None::<&str>,
            &[
                ("assignment_override[due_at]", due_at.unwrap_or_default()),
                ("assignment_override[lock_at]", lock_at.unwrap_or_default()),
            ],
            token,
        )
        .await?
        .error_for_status()?;
        Ok(())
    }

    pub async fn delete_assignment_ddl_override(
        &self,
        course_id: i64,
        assignment_id: i64,
        override_id: i64,
        token: &str,
    ) -> Result<()> {
        let url = format!(
            "{}/api/v1/courses/{}/assignments/{}/overrides/{}",
            BASE_URL, course_id, assignment_id, override_id
        );
        self.cli
            .delete(url)
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn add_assignment_ddl_override(
        &self,
        course_id: i64,
        assignment_id: i64,
        student_id: i64,
        title: &str,
        due_at: Option<&str>,
        lock_at: Option<&str>,
        token: &str,
    ) -> Result<()> {
        let url = format!(
            "{}/api/v1/courses/{}/assignments/{}/overrides",
            BASE_URL, course_id, assignment_id
        );
        self.post_form_with_token(
            &url,
            None::<&str>,
            &[
                (
                    "assignment_override[student_ids][]",
                    student_id.to_string().deref(),
                ),
                ("assignment_override[title]", title),
                ("assignment_override[due_at]", due_at.unwrap_or_default()),
                ("assignment_override[lock_at]", lock_at.unwrap_or_default()),
            ],
            token,
        )
        .await?
        .error_for_status()?;
        Ok(())
    }

    pub async fn get_file_content(file: &File) -> Result<Bytes> {
        let response = reqwest::Client::new()
            .get(&file.url)
            .send()
            .await?
            .error_for_status()?;
        let bytes = response.bytes().await?;
        Ok(bytes)
    }

    pub async fn download_file<F: Fn(ProgressPayload) + Send>(
        &self,
        file: &File,
        token: &str,
        save_path: &str,
        progress_handler: F,
    ) -> Result<()> {
        let mut response = self
            .get_request_with_token(&file.url, None::<&str>, token)
            .await?
            .error_for_status()?;

        let mut payload = ProgressPayload {
            uuid: file.uuid.clone(),
            processed: 0,
            total: file.size,
        };
        let path = Path::new(save_path).join(&file.display_name);
        let total = file.size;
        let mut file = fs::File::create(path.to_str().unwrap())?;
        let mut last_chunk_no = 0;
        while let Some(chunk) = response.chunk().await? {
            payload.processed += chunk.len() as u64;
            let chunk_no = payload.processed / CHUNK_SIZE;
            if chunk_no != last_chunk_no || payload.processed == total {
                last_chunk_no = chunk_no;
                progress_handler(payload.clone());
            }
            file.write_all(&chunk)?;
        }

        tracing::info!("File downloaded successfully!");
        Ok(())
    }

    pub async fn list_items_with_page<T: DeserializeOwned>(
        &self,
        url: &str,
        token: &str,
        page: u64,
    ) -> Result<Vec<T>> {
        let items = self
            .get_json_with_token(
                url,
                Some(&vec![
                    ("page", page.to_string()),
                    ("per_page", "100".to_owned()),
                ]),
                token,
            )
            .await?;
        Ok(items)
    }

    pub async fn list_items<T: DeserializeOwned>(&self, url: &str, token: &str) -> Result<Vec<T>> {
        let mut all_items = vec![];
        let mut page = 1;

        loop {
            let items = self.list_items_with_page(url, token, page).await?;
            if items.is_empty() {
                break;
            }
            page += 1;
            all_items.extend(items);
        }
        Ok(all_items)
    }

    pub async fn list_course_files(&self, course_id: i64, token: &str) -> Result<Vec<File>> {
        let url = format!("{}/api/v1/courses/{}/files", BASE_URL, course_id);
        self.list_items(&url, token).await
    }

    pub async fn list_course_images(&self, course_id: i64, token: &str) -> Result<Vec<File>> {
        let url = format!(
            "{}/api/v1/courses/{}/files?content_types[]=image",
            BASE_URL, course_id
        );
        self.list_items(&url, token).await
    }

    pub async fn list_folder_files(&self, folder_id: i64, token: &str) -> Result<Vec<File>> {
        let url = format!("{}/api/v1/folders/{}/files", BASE_URL, folder_id);
        self.list_items(&url, token).await
    }

    // TODO: 将接口改为list_course_folders
    pub async fn list_folders(&self, course_id: i64, token: &str) -> Result<Vec<Folder>> {
        let url = format!("{}/api/v1/courses/{}/folders", BASE_URL, course_id);
        self.list_items(&url, token).await
    }

    pub async fn list_folder_folders(&self, folder_id: i64, token: &str) -> Result<Vec<Folder>> {
        let url = format!("{}/api/v1/folders/{}/folders", BASE_URL, folder_id);
        self.list_items(&url, token).await
    }

    pub async fn get_folder_by_id(&self, folder_id: i64, token: &str) -> Result<Folder> {
        let url = format!("{}/api/v1/folders/{}", BASE_URL, folder_id);
        let folder = self.get_json_with_token(&url, None::<&str>, token).await?;
        Ok(folder)
    }

    pub async fn get_colors(&self, token: &str) -> Result<Colors> {
        let url = format!("{}/api/v1/users/self/colors", BASE_URL);
        let colors = self.get_json_with_token(&url, None::<&str>, token).await?;
        Ok(colors)
    }

    pub async fn list_calendar_events_inner(
        &self,
        token: &str,
        context_codes: &[String],
        start_date: &str,
        end_date: &str,
    ) -> Result<Vec<CalendarEvent>> {
        let context_codes = context_codes
            .iter()
            .map(|context_code| format!("context_codes[]={}", context_code))
            .reduce(|c1, c2| format!("{}&{}", c1, c2))
            .unwrap_or_default();
        let url = format!(
            "{}/api/v1/calendar_events?type=assignment&{}&start_date={}&end_date={}",
            BASE_URL, context_codes, start_date, end_date
        );
        self.list_items(&url, token).await
    }

    pub async fn list_calendar_events(
        &self,
        token: &str,
        context_codes: &[String],
        start_date: &str,
        end_date: &str,
    ) -> Result<Vec<CalendarEvent>> {
        const BATCH_SIZE: usize = 10;
        let n_codes = context_codes.len();
        let n_batches = if n_codes % BATCH_SIZE == 0 {
            n_codes / BATCH_SIZE
        } else {
            n_codes / BATCH_SIZE + 1
        };

        let mut start;
        let mut end;
        let mut all_events = vec![];
        for batch_idx in 0..n_batches {
            start = batch_idx * BATCH_SIZE;
            end = min(start + BATCH_SIZE, n_codes);
            let context_codes_batch = &context_codes[start..end];
            let events = self
                .list_calendar_events_inner(token, context_codes_batch, start_date, end_date)
                .await?;
            all_events.extend(events);
        }
        Ok(all_events)
    }

    pub async fn list_course_users(&self, course_id: i64, token: &str) -> Result<Vec<User>> {
        let url = format!("{}/api/v1/courses/{}/users", BASE_URL, course_id);
        self.list_items(&url, token).await
    }

    pub async fn get_single_course_assignment_submission(
        &self,
        course_id: i64,
        assignment_id: i64,
        student_id: i64,
        token: &str,
    ) -> Result<Submission> {
        let url = format!(
            "{}/api/v1/courses/{}/assignments/{}/submissions/{}?include[]=submission_comments",
            BASE_URL, course_id, assignment_id, student_id,
        );
        let submission = self.get_json_with_token(&url, None::<&str>, token).await?;
        Ok(submission)
    }

    pub async fn list_course_assignment_submissions(
        &self,
        course_id: i64,
        assignment_id: i64,
        token: &str,
    ) -> Result<Vec<Submission>> {
        let url = format!(
            "{}/api/v1/courses/{}/assignments/{}/submissions?include[]=submission_comments",
            BASE_URL, course_id, assignment_id
        );
        self.list_items(&url, token).await
    }

    pub async fn list_course_students(&self, course_id: i64, token: &str) -> Result<Vec<User>> {
        let url = format!("{}/api/v1/courses/{}/students", BASE_URL, course_id);
        self.list_items_with_page(&url, token, 0).await
    }

    pub async fn list_courses(&self, token: &str) -> Result<Vec<Course>> {
        let url = format!(
            "{}/api/v1/courses?include[]=teachers&include[]=term",
            BASE_URL
        );
        let all_courses = self.list_items(&url, token).await?;
        let filtered_courses: Vec<Course> = all_courses
            .into_iter()
            .filter(|course: &Course| !course.is_access_restricted())
            .collect();
        Ok(filtered_courses)
    }

    pub async fn get_me(&self, token: &str) -> Result<User> {
        let url = format!("{}/api/v1/users/self", BASE_URL);
        let me = self.get_json_with_token(&url, None::<&str>, token).await?;
        Ok(me)
    }

    async fn upload_submission_file_with(
        &self,
        params: &SubmissionUploadSuccessResponse,
        file_path: &str,
    ) -> Result<File> {
        let upload_params = &params.upload_params;
        let file_fs = fs::read(file_path)?;
        let file = multipart::Part::bytes(file_fs).file_name("filename.filetype");
        let form = reqwest::multipart::Form::new()
            .text("x-amz-credential", upload_params.x_amz_credential.clone())
            .text("x-amz-algorithm", upload_params.x_amz_algorithm.clone())
            .text("x-amz-date", upload_params.x_amz_date.clone())
            .text("x-amz-signature", upload_params.x_amz_signature.clone())
            .text("Filename", upload_params.filename.clone())
            .text("key", upload_params.key.clone())
            .text("acl", upload_params.acl.clone())
            .text("Policy", upload_params.policy.clone())
            .text(
                "success_action_redirect",
                upload_params.success_action_redirect.clone(),
            )
            .text("content-type", upload_params.content_type.clone())
            .part("file", file);

        let resp = self
            .cli
            .post(&params.upload_url)
            .multipart(form)
            .send()
            .await?
            .error_for_status()?;

        let bytes = resp.bytes().await?;
        let file = serde_json::from_slice(&bytes)?;
        Ok(file)
    }

    async fn prepare_upload_submission_file(
        &self,
        course_id: i64,
        assignment_id: i64,
        file_path: &str,
        file_name: &str,
        token: &str,
    ) -> Result<SubmissionUploadSuccessResponse> {
        let url = format!(
            "{}/api/v1/courses/{}/assignments/{}/submissions/self/files",
            BASE_URL, course_id, assignment_id,
        );
        let metadata = fs::metadata(file_path)?;
        if !metadata.is_file() {
            let error_message = format!("{} is not a valid file!", file_path);
            return Err(ClientError::SubmissionUpload(error_message));
        }

        let form = [("name", file_name), ("size", &metadata.len().to_string())];
        let resp = self
            .post_form_with_token(&url, None::<&str>, &form, token)
            .await?;
        let bytes = resp.bytes().await?;
        let result = match serde_json::from_slice::<SubmissionUploadResult>(&bytes)? {
            SubmissionUploadResult::Success(success_response) => success_response,
            SubmissionUploadResult::Error(error_response) => {
                return Err(ClientError::SubmissionUpload(error_response.message))
            }
        };
        Ok(result)
    }

    pub async fn submit_assignment(
        &self,
        course_id: i64,
        assignment_id: i64,
        file_paths: &[String],
        comment: Option<&str>,
        token: &str,
    ) -> Result<()> {
        let mut file_ids = vec![];
        for file_path in file_paths {
            let file_name = file_path.split('/').last().unwrap();
            let file = self
                .upload_submission_file(course_id, assignment_id, file_path, file_name, token)
                .await?;
            file_ids.push(file.id);
        }

        let url = format!(
            "{}/api/v1/courses/{}/assignments/{}/submissions",
            BASE_URL, course_id, assignment_id,
        );
        let mut form = vec![("submission[submission_type]", "online_upload".to_owned())];
        for file_id in file_ids {
            form.push(("submission[file_ids][]", file_id.to_string()));
        }
        if let Some(comment) = comment {
            form.push(("comment[text_comment]", comment.to_owned()));
        }
        self.post_form_with_token(&url, None::<&str>, &form, token)
            .await?;
        Ok(())
    }

    // Reference: https://canvas.instructure.com/doc/api/file.file_uploads.html
    pub async fn upload_submission_file(
        &self,
        course_id: i64,
        assignment_id: i64,
        file_path: &str,
        file_name: &str,
        token: &str,
    ) -> Result<File> {
        // Step 1: Telling Canvas about the file upload and getting a token
        let params = self
            .prepare_upload_submission_file(course_id, assignment_id, file_path, file_name, token)
            .await?;
        // Step 2: Upload the file data to the URL given in the previous response
        let file = self.upload_submission_file_with(&params, file_path).await?;
        Ok(file)
    }

    pub async fn list_ta_courses(&self, token: &str) -> Result<Vec<Course>> {
        let url = format!(
            "{}/api/v1/courses?include[]=teachers&include[]=term&enrollment_type=ta",
            BASE_URL
        );
        self.list_items(&url, token).await
    }

    pub async fn get_my_single_submission(
        &self,
        course_id: i64,
        assignment_id: i64,
        token: &str,
    ) -> Result<Submission> {
        let url = format!(
            "{}/api/v1/courses/{}/assignments/{}/submissions/self?include[]=submission_comments",
            BASE_URL, course_id, assignment_id,
        );
        let submission = self.get_json_with_token(&url, None::<&str>, token).await?;
        Ok(submission)
    }

    pub async fn list_course_assignments(
        &self,
        course_id: i64,
        token: &str,
    ) -> Result<Vec<Assignment>> {
        let url = format!(
            "{}/api/v1/courses/{}/assignments?include[]=submission&include[]=overrides&include[]=all_dates",
            BASE_URL, course_id
        );
        self.list_items(&url, token).await
    }
}

// Apis here are for course video
// We take references from: https://github.com/prcwcy/sjtu-canvas-video-download/blob/master/sjtu_canvas_video.py
impl Client {
    pub fn init_cookie(&self, cookie: &str) {
        self.jar
            .add_cookie_str(cookie, &Url::parse(VIDEO_BASE_URL).unwrap());
    }

    async fn get_request<T: Serialize + ?Sized>(
        &self,
        url: &str,
        query: Option<&T>,
    ) -> Result<Response> {
        let mut req = self.cli.get(url);

        if let Some(query) = query {
            req = req.query(query);
        }

        let res = req.send().await?;
        Ok(res)
    }

    async fn get_json_with_cookie<T: Serialize + ?Sized, D: DeserializeOwned>(
        &self,
        url: &str,
        query: Option<&T>,
    ) -> Result<D> {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        let mut req = self.cli.get(url).headers(headers);

        if let Some(query) = query {
            req = req.query(query);
        }

        let response = req.send().await?.error_for_status()?;
        let json = serde_json::from_slice(&response.bytes().await?)?;
        Ok(json)
    }

    pub async fn get_uuid(&self) -> Result<Option<String>> {
        let resp = self.cli.get(MY_SJTU_URL).send().await?.error_for_status()?;
        let body = resp.text().await?;
        // let document = Document::from(body.as_str());
        let re = Regex::new(
            r#"uuid=([0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})"#,
        )
        .unwrap();

        if let Some(captures) = re.captures(&body) {
            if let Some(uuid) = captures.get(1) {
                return Ok(Some(uuid.as_str().to_owned()));
            }
        }

        Ok(None)
    }

    pub async fn express_login(&self, uuid: &str) -> Result<Option<String>> {
        let url = format!("{}?uuid={}", EXPRESS_LOGIN_URL, uuid);
        self.cli.get(&url).send().await?.error_for_status()?;
        let domain = Url::parse(AUTH_URL).unwrap();
        if let Some(value) = self.jar.cookies(&domain) {
            if let Ok(cookies) = value.to_str() {
                let kvs = cookies.split(';');
                for kv in kvs {
                    let kv: Vec<_> = kv.trim().split('=').collect();
                    if kv.len() >= 2 && kv[0] == "JAAuthCookie" {
                        return Ok(Some(kv[1].to_owned()));
                    }
                }
            }
        }
        Ok(None)
    }

    pub async fn login_video_website(&self, cookie: &str) -> Result<Option<String>> {
        self.jar
            .add_cookie_str(cookie, &Url::parse(AUTH_URL).unwrap());
        let response = self.get_request(VIDEO_LOGIN_URL, None::<&str>).await?;
        let url = response.url();
        if let Some(domain) = url.domain() {
            if domain == "jaccount.sjtu.edu.cn" {
                return Err(ClientError::LoginError);
            }
        }
        if let Some(cookies) = self.jar.cookies(&Url::parse(VIDEO_BASE_URL).unwrap()) {
            if let Ok(cookies) = cookies.to_str() {
                return Ok(Some(cookies.to_owned()));
            }
        }
        Ok(None)
    }

    pub async fn get_page_items<T: Serialize + DeserializeOwned>(
        &self,
        url: &str,
    ) -> Result<Vec<T>> {
        let mut page_index = 1;
        let mut all_items = vec![];

        loop {
            let paged_url = format!("{}pageSize=100&pageIndex={}", url, page_index);
            let item_page = self
                .get_json_with_cookie::<_, ItemPage<T>>(&paged_url, None::<&str>)
                .await?;
            all_items.extend(item_page.list);
            let page = &item_page.page;
            if page.page_count == 0 || page.page_next == page_index {
                break;
            }
            page_index += 1;
        }
        Ok(all_items)
    }

    pub async fn get_subjects(&self) -> Result<Vec<Subject>> {
        let url = format!(
            "{}/system/course/subject/findSubjectVodList?",
            VIDEO_BASE_URL
        );
        self.get_page_items(&url).await
    }

    pub async fn get_oauth_consumer_key(&self) -> Result<Option<String>> {
        let resp = self.get_request(VIDEO_OAUTH_KEY_URL, None::<&str>).await?;
        let body = resp.text().await?;
        let document = Document::from(body.as_str());

        let Some(meta) = document
            .find(Name("meta"))
            .find(|n: &Node| n.attr("id").unwrap_or_default() == "xForSecName")
        else {
            return Ok(None);
        };
        let Some(v) = meta.attr("vaule") else {
            return Ok(None);
        };
        let bytes = &STANDARD.decode(v)?;
        Ok(Some(format!("{}", String::from_utf8_lossy(bytes))))
    }

    pub async fn get_video_course(
        &self,
        subject_id: i64,
        tecl_id: i64,
    ) -> Result<Option<VideoCourse>> {
        let url = format!(
            "{}/system/resource/vodVideo/getCourseListBySubject?orderField=courTimes&subjectId={}&teclId={}&",
            VIDEO_BASE_URL, subject_id, tecl_id
        );
        let mut courses = self.get_page_items(&url).await?;
        Ok(courses.remove(0))
    }

    fn get_oauth_signature(
        &self,
        video_id: i64,
        oauth_nonce: &str,
        oauth_consumer_key: &str,
    ) -> String {
        let signature_string = format!("/app/system/resource/vodVideo/getvideoinfos?id={}&oauth-consumer-key={}&oauth-nonce={}&oauth-path={}&{}&playTypeHls=true",
        video_id, oauth_consumer_key, oauth_nonce, OAUTH_PATH, OAUTH_RANDOM);
        let md5 = Md5::digest(signature_string);
        format!("{:x}", md5)
    }

    fn get_oauth_nonce(&self) -> String {
        let now = SystemTime::now();
        let since_the_epoch = now.duration_since(UNIX_EPOCH).expect("Time went backwards");
        (since_the_epoch.as_nanos() / 1_000_000).to_string()
    }

    async fn download_video_partial(&self, url: &str, begin: u64, end: u64) -> Result<Response> {
        let range_value = HeaderValue::from_str(&format!("bytes={}-{}", begin, end)).unwrap();
        let response = self
            .cli
            .get(url)
            .header(RANGE, range_value)
            .header(REFERER, "https://courses.sjtu.edu.cn")
            .send()
            .await?;
        Ok(response)
    }

    async fn get_download_video_size(&self, url: &str) -> Result<u64> {
        let resp = self.download_video_partial(url, 0, 0).await?;
        let range = resp.headers().get(CONTENT_RANGE);
        if let Some(range) = range {
            let range = range.to_str()?;
            let parts: Vec<_> = range.split('/').collect();
            let size = if parts.len() == 2 {
                parts[1].parse().unwrap_or_default()
            } else {
                0
            };
            Ok(size)
        } else {
            Ok(0)
        }
    }

    pub async fn download_video<F: Fn(ProgressPayload) + Send>(
        &self,
        video: &VideoPlayInfo,
        save_path: &str,
        progress_handler: F,
    ) -> Result<()> {
        let mut output_file = fs::File::create(save_path)?;
        let mut read_total = 0_u64;
        let url = &video.rtmp_url_hdv;
        let size = self.get_download_video_size(url).await?;
        let mut payload = ProgressPayload {
            uuid: video.id.to_string(),
            processed: 0,
            total: size,
        };
        progress_handler(payload.clone());
        loop {
            let response = self
                .download_video_partial(url, read_total, read_total + VIDEO_CHUNK_SIZE)
                .await?;

            let status = response.status();
            if !(status == StatusCode::OK || status == StatusCode::PARTIAL_CONTENT) {
                tracing::error!("status not ok: {}", status);
            }
            let bytes = response.bytes().await?;
            let read_bytes = bytes.len() as u64;
            tracing::debug!("read bytes {}", read_bytes);

            output_file.write_all(&bytes)?;
            read_total += read_bytes;
            payload.processed += read_bytes;
            progress_handler(payload.clone());
            if read_bytes < VIDEO_CHUNK_SIZE {
                break;
            }
        }
        tracing::debug!("read total bytes {}", read_total);
        Ok(())
    }

    pub async fn get_video_info(
        &self,
        video_id: i64,
        oauth_consumer_key: &str,
    ) -> Result<VideoInfo> {
        let mut form_data = HashMap::new();
        let oauth_nonce = self.get_oauth_nonce();
        let oauth_signature = self.get_oauth_signature(video_id, &oauth_nonce, oauth_consumer_key);

        tracing::debug!("oauth_nonce: {}", oauth_nonce);
        tracing::debug!("oauth_signature: {}", oauth_signature);
        tracing::debug!("oauth_consumer_key: {}", oauth_consumer_key);
        tracing::debug!("video_id: {}", video_id);

        let video_id_str = video_id.to_string();
        form_data.insert("playTypeHls", "true");
        form_data.insert("id", &video_id_str);
        form_data.insert(OAUTH_RANDOM_P1, OAUTH_RANDOM_P1_VAL);
        form_data.insert(OAUTH_RANDOM_P2, OAUTH_RANDOM_P2_VAL);

        let response = self
            .cli
            .post(VIDEO_INFO_URL)
            .form(&form_data)
            .header(ACCEPT, "application/json")
            .header("oauth-consumer-key", oauth_consumer_key)
            .header("oauth-nonce", oauth_nonce)
            .header("oauth-path", OAUTH_PATH)
            .header("oauth-signature", oauth_signature)
            .send()
            .await?
            .error_for_status()?;
        let bytes = response.bytes().await?;
        let video = serde_json::from_slice(&bytes)?;
        Ok(video)
    }
}

// Apis here are for jbox
// Check https://pan.sjtu.edu.cn/
impl Client {
    async fn post_request<D: DeserializeOwned, B: Into<Body>>(
        &self,
        url: &str,
        body: B,
    ) -> Result<D> {
        let req = self
            .cli
            .post(url)
            .body(body)
            .header(CONTENT_TYPE, "application/json");
        let resp = req.send().await?.error_for_status()?;
        let bytes = resp.bytes().await?;
        let result = serde_json::from_slice(&bytes)?;
        Ok(result)
    }

    pub async fn login_jbox(&self, cookie: &str) -> Result<String> {
        self.jar
            .add_cookie_str(cookie, &Url::parse(AUTH_URL).unwrap());
        let resp = self
            .get_request(JBOX_LOGIN_URL, None::<&str>)
            .await?
            .error_for_status()?;
        let re = Regex::new(r"code=(.+?)&state=").unwrap();
        let url = resp.url().to_string();
        let Some(captures) = re.captures(&url) else {
            return Err(ClientError::LoginError);
        };

        let Some(m) = captures.get(1) else {
            return Err(ClientError::LoginError);
        };

        let code = m.as_str().to_owned();
        let next_url = format!("{}{}", JBOX_LOGIN_URL2, code);
        let login_result = self
            .post_request::<JboxLoginResult, _>(&next_url, "")
            .await?;

        if login_result.status != 0 || login_result.user_token.len() != 128 {
            return Err(ClientError::LoginError);
        }
        Ok(login_result.user_token)
    }

    pub async fn get_user_space_info(&self, user_token: &str) -> Result<PersonalSpaceInfo> {
        let url = format!("{}?user_token={}", JBOX_USER_SPACE_URL, user_token);
        let info = self.post_request::<PersonalSpaceInfo, _>(&url, "").await?;
        if info.status != 0 {
            tracing::error!("{}", info.message);
            return Err(ClientError::JBoxError(info.message));
        }
        Ok(info)
    }

    pub async fn start_chunk_upload(
        &self,
        path: &str,
        chunk_count: usize,
        info: &JBoxLoginInfo,
    ) -> Result<StartChunkUploadContext> {
        let url = format!(
            "{}/api/v1/file/{}/{}/{}?multipart=null&conflict_resolution_strategy=rename&access_token={}",
            JBOX_BASE_URL, info.library_id, info.space_id, path, info.access_token
        );
        let chunks: Vec<_> = (1..=chunk_count).map(usize::from).collect();
        let data = json!({"partNumberRange": chunks}).to_string();
        let result = self
            .post_request::<StartChunkUploadContext, _>(&url, data)
            .await?;
        Ok(result)
    }

    pub async fn create_jbox_directory(&self, dir_path: &str, info: &JBoxLoginInfo) -> Result<()> {
        let url = format!(
            "{}/api/v1/directory/{}/{}/{}?conflict_resolution_strategy=ask&access_token={}",
            JBOX_BASE_URL, info.library_id, info.space_id, dir_path, info.access_token
        );
        let resp = self.cli.put(&url).send().await?;
        let bytes = resp.bytes().await?;
        let result = serde_json::from_slice::<JBoxErrorMessage>(&bytes)?;
        if result.status != 0 && result.code != "SameNameDirectoryOrFileExists" {
            return Err(ClientError::JBoxError(result.message));
        }
        Ok(())
    }

    pub fn compute_chunk_size(&self, file_size: usize) -> usize {
        if file_size % JBOX_UPLOAD_CHUNK_SIZE == 0 {
            file_size / JBOX_UPLOAD_CHUNK_SIZE
        } else {
            file_size / JBOX_UPLOAD_CHUNK_SIZE + 1
        }
    }

    async fn upload_chunk(
        &self,
        ctx: &StartChunkUploadContext,
        data: Vec<u8>,
        part_number: usize,
    ) -> Result<()> {
        let url = format!(
            "https://{}{}?uploadId={}&partNumber={}",
            ctx.domain, ctx.path, ctx.upload_id, part_number
        );
        let headers = &ctx
            .parts
            .get(&part_number.to_string())
            .ok_or(ClientError::JBoxError("非法 Part 结构".to_owned()))?
            .headers;
        self.cli
            .put(&url)
            .header(ACCEPT, "*/*")
            .header(ACCEPT_ENCODING, "gzip, deflate, br")
            .header(ACCEPT_LANGUAGE, "zh-CN,zh;q=0.9,en-US;q=0.8,en;q=0.7")
            .header("x-amz-date", &headers.x_amz_date)
            .header("authorization", &headers.authorization)
            .header("x-amz-content-sha256", &headers.x_amz_content_sha256)
            .body(data)
            .send()
            .await?
            .error_for_status()?;
        tracing::info!("upload chunk: {}", part_number);
        Ok(())
    }

    async fn upload_chunk_with_retry(
        &self,
        ctx: &StartChunkUploadContext,
        data: &[u8],
        part_number: usize,
        max_retries: i64,
    ) -> Result<()> {
        let mut retries = 0;
        let mut result;
        loop {
            result = self.upload_chunk(ctx, data.to_owned(), part_number).await;
            if result.is_ok() || retries == max_retries {
                break;
            }
            retries += 1;
        }
        result
    }

    async fn confirm_chunk_upload(&self, confirm_key: &str, info: &JBoxLoginInfo) -> Result<()> {
        let url = format!(
            "{}/api/v1/file/{}/{}/{}?confirm=null&conflict_resolution_strategy=rename&access_token={}",
            JBOX_BASE_URL, info.library_id, info.space_id, confirm_key, info.access_token
        );
        let result = self
            .post_request::<ConfirmChunkUploadResult, _>(&url, "")
            .await?;
        tracing::info!("上传成功！crc64 = {}", result.crc64);
        Ok(())
    }

    pub async fn upload_file<F: Fn(ProgressPayload) + Send>(
        &self,
        file: &File,
        save_dir: &str,
        info: &JBoxLoginInfo,
        progress_handler: F,
    ) -> Result<()> {
        // ensure directory exists
        self.create_jbox_directory(save_dir, info).await?;

        let save_path = Path::new(save_dir).join(&file.display_name);
        let response = self
            .get_request(&file.url, None::<&str>)
            .await?
            .error_for_status()?;
        let data = response.bytes().await?.to_vec();
        let file_size = data.len();
        let chunk_count = self.compute_chunk_size(file_size);
        let ctx = self
            .start_chunk_upload(save_path.to_str().unwrap(), chunk_count, info)
            .await?;
        let mut payload = ProgressPayload {
            uuid: file.uuid.clone(),
            processed: 0,
            total: file_size as u64,
        };
        for part_number in 1..=chunk_count {
            let start = (part_number - 1) * JBOX_UPLOAD_CHUNK_SIZE;
            let end = min(start + JBOX_UPLOAD_CHUNK_SIZE, file_size);
            let this_chunk_size = end - start;
            self.upload_chunk_with_retry(&ctx, &data[start..end], part_number, 3)
                .await?;
            payload.processed += this_chunk_size as u64;
            progress_handler(payload.clone());
        }
        // confirm
        self.confirm_chunk_upload(&ctx.confirm_key, info).await?;
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use crate::{
        client::Client,
        error::Result,
        model::{Course, EnrollmentRole},
    };
    use std::collections::HashMap;

    fn os_env_hashmap() -> HashMap<String, String> {
        let mut map = HashMap::new();
        use std::env;
        for (key, val) in env::vars_os() {
            // Use pattern bindings instead of testing .is_some() followed by .unwrap()
            if let (Ok(k), Ok(v)) = (key.into_string(), val.into_string()) {
                map.insert(k, v);
            }
        }
        map
    }

    fn get_token_from_env() -> String {
        let env_vars = os_env_hashmap();
        env_vars.get("CANVAS_TOKEN").cloned().unwrap_or_default()
    }

    fn check_rfc3339_time_format(time: &Option<String>) -> bool {
        if let Some(time) = time {
            chrono::DateTime::parse_from_rfc3339(time).is_ok()
        } else {
            true
        }
    }

    fn is_ta(course: &Course) -> bool {
        let filtered: Vec<_> = course
            .enrollments
            .iter()
            .filter(|enrollment| enrollment.role == EnrollmentRole::TaEnrollment)
            .collect();
        !filtered.is_empty()
    }

    #[tokio::test]
    async fn test_get_uuid() -> Result<()> {
        let cli = Client::new();
        let uuid = cli.get_uuid().await?;
        assert!(uuid.is_some());
        let uuid: String = uuid.unwrap();
        assert!(!uuid.is_empty());
        Ok(())
    }

    #[test]
    fn test_get_oauth_signature() -> Result<()> {
        let cli = Client::new();
        let oauth_nonce = "1709784720392";
        let id = 3601811;
        let oauth_consumer_key = "DADD2CA9923D5E31331C4B79B39A1E4B";
        assert_eq!(
            "2b499a5303048d6522118e79711c5ee0",
            cli.get_oauth_signature(id, oauth_nonce, oauth_consumer_key)
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_get_me() -> Result<()> {
        let token = get_token_from_env();
        assert!(!token.is_empty());
        let cli = Client::new();
        let me = cli.get_me(&token).await?;
        assert!(me.id > 0);
        assert!(!me.name.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn test_list_courses() -> Result<()> {
        let token = get_token_from_env();
        assert!(!token.is_empty());
        let cli = Client::new();

        let courses = cli.list_courses(&token).await?;
        assert!(!courses.is_empty());
        for course in courses {
            assert!(course.id > 0);
            assert!(course.term.id > 0);
            assert!(!course.teachers.is_empty());
            assert!(!course.uuid.is_empty());
            assert!(!course.enrollments.is_empty());
            assert!(check_rfc3339_time_format(&course.term.created_at));
            assert!(check_rfc3339_time_format(&course.term.start_at));
            assert!(check_rfc3339_time_format(&course.term.end_at));
        }
        Ok(())
    }

    #[tokio::test]
    async fn test_list_assignments() -> Result<()> {
        tracing_subscriber::fmt::init();
        let token = get_token_from_env();
        assert!(!token.is_empty());
        let cli = Client::new();
        let courses = cli.list_courses(&token).await?;
        for course in courses {
            let assignments = cli.list_course_assignments(course.id, &token).await?;
            for assignment in assignments {
                assert_eq!(assignment.course_id, course.id);
                assert!(assignment.id > 0);
                assert!(check_rfc3339_time_format(&assignment.due_at));
                assert!(check_rfc3339_time_format(&assignment.lock_at));
                for assignment_override in assignment.overrides {
                    assert!(check_rfc3339_time_format(&assignment_override.unlock_at));
                    assert!(check_rfc3339_time_format(&assignment_override.lock_at));
                    assert!(check_rfc3339_time_format(&assignment_override.due_at));
                    assert!(!assignment_override.student_ids.is_empty());
                }
            }
        }
        Ok(())
    }

    #[tokio::test]
    async fn test_list_users() -> Result<()> {
        let token = get_token_from_env();
        assert!(!token.is_empty());
        let cli = Client::new();
        let courses = cli.list_courses(&token).await?;
        for course in courses {
            let term = &course.term;
            let Some(end_at) = &term.end_at else {
                continue;
            };
            let is_ta = is_ta(&course);
            let end_at = chrono::DateTime::parse_from_rfc3339(end_at);
            assert!(end_at.is_ok());
            let end_at = end_at.unwrap().naive_local();
            let now = chrono::offset::Local::now().naive_local();

            if now > end_at && !is_ta {
                assert!(cli.list_course_users(course.id, &token).await.is_err());
                continue;
            }
            let users = cli.list_course_users(course.id, &token).await?;

            assert!(!users.is_empty());

            for user in users {
                assert!(user.id > 0);
                assert!(!user.name.is_empty());
                if is_ta {
                    assert!(!user.email.is_empty());
                }
            }
        }
        Ok(())
    }

    #[tokio::test]
    async fn test_list_submissions() -> Result<()> {
        let token = get_token_from_env();
        assert!(!token.is_empty());
        let cli = Client::new();
        let courses = cli.list_courses(&token).await?;
        for course in courses {
            let is_ta = is_ta(&course);
            if !is_ta {
                continue;
            }

            let assignments = cli.list_course_assignments(course.id, &token).await?;
            for assignment in assignments {
                let submissions = cli
                    .list_course_assignment_submissions(course.id, assignment.id, &token)
                    .await?;
                for submission in submissions {
                    assert_eq!(submission.assignment_id, assignment.id);
                    assert!(submission.id > 0);
                    assert!(check_rfc3339_time_format(&submission.submitted_at));
                }
            }
        }
        Ok(())
    }
}
