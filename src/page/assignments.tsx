import { Checkbox, CheckboxProps, Divider, Space, Table, Tag } from "antd";
import BasicLayout from "../components/layout";
import useMessage from "antd/es/message/useMessage";
import { useEffect, useState } from "react";
import { Assignment, Attachment, Course, Submission } from "../lib/model";
import { invoke } from "@tauri-apps/api";
import { attachmentToFile, formatDate } from "../lib/utils";
import CourseSelect from "../components/course_select";
import { usePreview } from "../lib/hooks";
import dayjs from "dayjs";

export default function AssignmentsPage() {
    const [messageApi, contextHolder] = useMessage();
    const [operating, setOperating] = useState<boolean>(false);
    const [onlyShowUnfinished, setOnlyShowUnfinished] = useState<boolean>(true);
    const [courses, setCourses] = useState<Course[]>([]);
    const [assignments, setAssignments] = useState<Assignment[]>([]);
    const [selectedCourseId, setSelectedCourseId] = useState<number>(-1);
    const { previewer, onHoverEntry, onLeaveEntry, setPreviewEntry } = usePreview();
    const [linksMap, setLinksMap] = useState<Record<number, Attachment[]>>({});

    useEffect(() => {
        initCourses();
    }, []);

    const handleGetAssignments = async (courseId: number, onlyShowUnfinished: boolean) => {
        if (courseId === -1) {
            return;
        }
        setOperating(true);
        try {
            let linksMap = {};
            let assignments = await invoke("list_course_assignments", { courseId }) as Assignment[];
            assignments.map(assignment => assignment.key = assignment.id);
            if (onlyShowUnfinished) {
                assignments = assignments.filter(assignment => assignment.submission?.workflow_state === "unsubmitted")
            }
            for (let assignment of assignments) {
                dealWithDescription(assignment, linksMap);
            }
            setLinksMap(linksMap);
            setAssignments(assignments);
        } catch (e) {
            messageApi.error(e as string);
        }
        setOperating(false);
    }

    const initCourses = async () => {
        try {
            let courses = await invoke("list_courses") as Course[];
            setCourses(courses);
        } catch (e) {
            messageApi.error(e as string);
        }
    }

    const columns = [{
        title: '作业名',
        dataIndex: 'name',
        key: 'name',
        render: (_: any, assignment: Assignment) => <a href={assignment.html_url} target="_blank">{assignment.name}</a>
    }, {
        title: '开始时间',
        dataIndex: 'unlock_at',
        key: 'unlock_at',
        render: formatDate,
    }, {
        title: '截止时间',
        dataIndex: 'due_at',
        key: 'due_at',
        render: formatDate,
    }, {
        title: '结束时间',
        dataIndex: 'lock_at',
        key: 'lock_at',
        render: formatDate,
    }, {
        title: '得分',
        dataIndex: 'points_possible',
        key: 'points_possible',
        render: (points_possible: number | undefined, assignment: Assignment) => {
            let grade = assignment.submission?.grade ?? 0;
            if (points_possible) {
                return `${grade}/${points_possible}`;
            }
            return grade;
        }
    }, {
        title: '状态',
        dataIndex: 'submission',
        key: 'submission',
        render: (submission: Submission, assignment: Assignment) => {
            let tags = [];
            let dued = dayjs(assignment.due_at).isBefore(dayjs());
            let locked = dayjs(assignment.lock_at).isBefore(dayjs());
            if (dued || locked) {
                tags.push(<Tag color="orange">已截止</Tag>);
            } else {
                tags.push(<Tag color="blue">进行中</Tag>);
            }
            if (!submission ||
                assignment.submission_types.includes("none") || assignment.submission_types.includes("not_graded")) {
                // no need to submit
                tags.push(<Tag>无需提交</Tag>);
            }
            else if (submission.submitted_at) {
                tags.push(submission.late ? <Tag color="red">迟交</Tag> : <Tag color="green">已提交</Tag>);
            }
            else {
                tags.push(<Tag color="red">未提交</Tag>);
            }
            return <Space size={"small"}>{tags}</Space>
        }
    }];

    const handleDownloadAttachment = async (attachment: Attachment) => {
        let file = attachmentToFile(attachment);
        try {
            await invoke("download_file", { file });
            messageApi.success("下载成功🎉！", 0.5);
        } catch (e) {
            messageApi.success(`下载失败🥹(${e})！`);
        }
    }

    const attachmentColumns = [{
        title: '文件',
        dataIndex: 'display_name',
        key: 'display_name',
        render: (name: string, attachment: Attachment) => <a
            onMouseEnter={() => onHoverEntry(attachmentToFile(attachment))}
            onMouseLeave={onLeaveEntry}
        >
            {name}
        </a>
    }, {
        title: '操作',
        dataIndex: 'operation',
        key: 'operation',
        render: (_: any, attachment: Attachment) => (
            <Space>
                {attachment.url && <a onClick={e => {
                    e.preventDefault();
                    handleDownloadAttachment(attachment);
                }}>下载</a>}
                <a onClick={e => {
                    e.preventDefault();
                    setPreviewEntry(attachmentToFile(attachment));
                }}>预览</a>
            </Space>
        ),
    }];

    const submittedAttachmentColumns = [{
        title: '文件',
        dataIndex: 'display_name',
        key: 'display_name',
        render: (name: string, attachment: Attachment) => <a
            onMouseEnter={() => onHoverEntry(attachmentToFile(attachment))}
            onMouseLeave={onLeaveEntry}
        >
            {name}
        </a>
    }, {
        title: '提交时间',
        dataIndex: 'submitted_at',
        key: 'submitted_at',
        render: formatDate,
    }, {
        title: '状态',
        dataIndex: 'late',
        key: 'late',
        render: (late: boolean) => late ? <Tag color="red">迟交</Tag> : <Tag color="green">按时提交</Tag>
    }, {
        title: '操作',
        dataIndex: 'operation',
        key: 'operation',
        render: (_: any, attachment: Attachment) => (
            <Space>
                {attachment.url && <a onClick={e => {
                    e.preventDefault();
                    handleDownloadAttachment(attachment);
                }}>下载</a>}
                <a onClick={e => {
                    e.preventDefault();
                    setPreviewEntry(attachmentToFile(attachment));
                }}>预览</a>
            </Space>
        ),
    }];

    const handleCourseSelect = async (courseId: number) => {
        let selectedCourse = courses.find(course => course.id === courseId);
        if (selectedCourse) {
            setSelectedCourseId(courseId);
            handleGetAssignments(courseId, onlyShowUnfinished);
        }
    }

    const handleSetOnlyShowUnfinished: CheckboxProps['onChange'] = (e) => {
        let onlyShowUnfinished = e.target.checked;
        setOnlyShowUnfinished(onlyShowUnfinished);
        handleGetAssignments(selectedCourseId, onlyShowUnfinished);
    }

    const dealWithDescription = (assignment: Assignment, linksMap: Record<number, Attachment[]>) => {
        const parser = new DOMParser();
        const document = parser.parseFromString(assignment.description, "text/html");
        const anchorTags = document.querySelectorAll('a');
        const downloadableRegex = /https:\/\/oc\.sjtu\.edu\.cn\/courses\/(\d+)\/files\/(\d+)\/download\?/g;
        const id = assignment.id;
        if (!linksMap[id]) {
            linksMap[id] = [];
        }
        let links = linksMap[id];
        anchorTags.forEach(anchorTag => {
            // Set the target attribute of each anchor tag to "_blank"
            anchorTag.setAttribute("target", "_blank");
            if (anchorTag.href.match(downloadableRegex)) {
                links.push({ url: anchorTag.href, display_name: anchorTag.text, key: assignment.id } as Attachment);
            }
        });
        assignment.description = document.body.innerHTML;
    }

    return <BasicLayout>
        {contextHolder}
        {previewer}
        <Space direction="vertical" style={{ width: "100%", overflow: "scroll" }} size={"large"}>
            <CourseSelect onChange={handleCourseSelect} disabled={operating} courses={courses} />
            <Checkbox disabled={operating} onChange={handleSetOnlyShowUnfinished} defaultChecked>只显示未完成</Checkbox>
            <Table style={{ width: "100%" }}
                loading={operating}
                columns={columns}
                dataSource={assignments}
                pagination={false}
                expandable={{
                    expandedRowRender: (assignment) => {
                        let attachments = undefined;
                        let submission = assignment.submission;
                        if (submission) {
                            attachments = submission?.attachments;
                            attachments?.map(attachment => {
                                attachment.submitted_at = submission?.submitted_at;
                                attachment.key = attachment.id;
                            });
                        }
                        return <Space direction="vertical" style={{ width: "100%" }}>
                            <Divider orientation="left">作业描述</Divider>
                            <div dangerouslySetInnerHTML={{ __html: assignment.description }} />
                            <Divider orientation="left">作业附件</Divider>
                            <Table columns={attachmentColumns} dataSource={linksMap[assignment.id]} pagination={false} />
                            <Divider orientation="left">我的提交</Divider>
                            <Table columns={submittedAttachmentColumns} dataSource={attachments} pagination={false} />
                        </Space>
                    }
                }}
            />
        </Space>
    </BasicLayout>
}
