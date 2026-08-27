import { useState } from "react";
import { checkAppUpdate } from "../api";
import type { RuntimeSnapshot } from "../types";
import { Button, SectionLabel } from "../components/Ui";

export function HelpPage({runtime}:{runtime:RuntimeSnapshot}) {
  const [update,setUpdate]=useState("检查更新");const check=async()=>{setUpdate("正在检查…");const r=await checkAppUpdate();setUpdate(r.ok&&r.data?.available?`发现 ${r.data.latest}`:"当前已是最新版本")};
  return <div className="page help-page"><div className="help-grid"><section><SectionLabel index="01">快速开始</SectionLabel><p>快速开始指南</p><ol><li><span>01</span>将光标放到要输入的应用或输入框中</li><li><span>02</span>按语音输入快捷键开始说话</li><li><span>03</span>根据触发方式松开或再次按下快捷键结束，文字会自动输入</li></ol></section><section><SectionLabel index="02">数据与隐私</SectionLabel><p>历史记录和设置保存在这台电脑；语音会发送到云端进行识别。</p><ul><li>历史记录、纠错规则和设置保存在本机，可随时清空或导出。</li><li>语音识别需要联网；登录后，热词会同步到语音服务以提高准确率。</li><li>你可以随时调整悬浮窗、快捷键和输入方式。</li></ul></section></div><section className="update-section"><SectionLabel index="03">应用更新</SectionLabel><p>检查并安装 EasyInput 的新版本；安装前会征求你的确认。</p><div><span>当前版本 {runtime.version}<small>Intel Mac · x86_64</small></span><Button onClick={check}>{update}</Button></div></section></div>;
}
