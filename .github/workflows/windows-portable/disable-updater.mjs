import fs from "fs-extra";

async function disableUpdater() {
    const configPath = "./src-tauri/src-tauri/tauri.conf.json";
    if (!(await fs.pathExists(configPath))) {
        throw new Error("could not found the tauri.conf.json");
    }
    
    // backup tauri.conf.json
    await fs.copyFile(configPath, configPath + ".bak");
    // load config
    const config = await fs.readJson(configPath);
    // modify config
    config.updater = {
        active: false,
        dialog: false
    };
    config.bundle = {
        active: false
    };
    // save modified config
    await fs.writeJson(configPath, config);
}

disableUpdater().catch((err) => { console.error(err); });