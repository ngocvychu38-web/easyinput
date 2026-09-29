# T04 · Tauri 开发运行与 Keychain 身份稳定

- **背景**：当前配置文件已升级到版本 6，但旧打包应用仍在后台运行并只支持版本 5；与此同时，`tauri dev` 的 Cargo 原始二进制使用随构建变化的临时签名标识，导致开发实例读取既有 Keychain 项时表现不稳定。
- **目标**：确保开发启动前发现旧打包实例；确保 Cargo 在执行调试二进制前使用稳定的开发标识和 designated requirement 完成临时签名。
- **产出**：`scripts/dev.sh`、`scripts/codesign-dev-runner.sh`、相关文档和回归记录。
- **验收标准**：旧 bundle 正在运行时开发脚本给出明确提示并退出；正常启动时 runner 在二进制执行前完成签名；签名 Identifier 为 `pro.easyinput.desktop.intel`；当前配置版本 6 能读取；Keychain 四项元数据均存在。
- **状态**：已完成
