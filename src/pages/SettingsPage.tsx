import { useState } from "react";
import { updateAppSettings } from "../api";
import type { AppSettings, RuntimeSnapshot } from "../types";
import { Button, SectionLabel, SettingRow, Toggle } from "../components/Ui";

export function SettingsPage({ runtime, refresh }: { runtime: RuntimeSnapshot; refresh(): Promise<void> }) {
  const [settings,setSettings]=useState<AppSettings>(runtime.settings); const [status,setStatus]=useState("");
  const patch=<K extends keyof AppSettings>(key:K,value:AppSettings[K])=>setSettings({...settings,[key]:value});
  const save=async()=>{setStatus("保存中…");const result=await updateAppSettings(settings);if(result.ok){setStatus("已保存");await refresh()}else setStatus(result.message??"保存失败")};
  return <div className="page settings-page"><div className="settings-columns">
    <div><SectionLabel index="01">快捷键</SectionLabel>
      <SettingRow title="语音输入快捷键" hint="按住快捷键开始语音输入" action={<button className="hotkey">⌘ 右 Command</button>}/>
      <SettingRow title="编辑指令快捷键" hint="选中文字后，按住快捷键说出修改要求" action={<button className="hotkey">⌥ 右 Option</button>}/>
      <div className="setting-block"><b>快捷键操作方式</b><div className="segmented wide"><button className={settings.triggerMode==="Hold"?"active":""} onClick={()=>patch("triggerMode","Hold")}>按住说话，松开结束</button><button className={settings.triggerMode==="Toggle"?"active":""} onClick={()=>patch("triggerMode","Toggle")}>按一下开始，再按一下结束</button></div></div>
      <SectionLabel index="02">文字整理</SectionLabel><div className="setting-block"><div className="cleanup-head"><b>整理方式</b><div className="pills">{([['Original','原样输出'],['Smart','智能整理'],['Custom','自定义']] as const).map(([id,label])=><button key={id} className={settings.cleanupMode===id?"active":""} onClick={()=>patch("cleanupMode",id)}>{label}</button>)}</div></div><p className="hint">{settings.cleanupMode==="Original"?"保留口语表达，只应用你保存的纠错规则。":settings.cleanupMode==="Smart"?"自动修正明确口误，并在不改变原意的前提下适当分段。":"按你填写的语气和排版要求整理。"}</p>{settings.cleanupMode==="Custom"&&<textarea value={settings.customCleanup} onChange={e=>patch("customCleanup",e.target.value)} placeholder="例如：语气简洁、保留技术术语…"/>}</div>
    </div>
    <div><SectionLabel index="03">文字格式</SectionLabel><div className="format-card"><b>文字输入方式</b><p className="hint">自动选择适合当前输入框的方式；多行文字会一次粘贴。</p><div className="segmented wide">{([['Auto','自动（推荐）'],['Direct','直接输入（仅单行）'],['Paste','始终粘贴']] as const).map(([id,label])=><button className={settings.inputMode===id?"active":""} key={id} onClick={()=>patch("inputMode",id)}>{label}</button>)}</div></div>
      <SettingRow title="Enter 键停止录音" hint="录音时按 Enter 结束；文字输入完成后自动按一次回车" action={<Toggle label="Enter键停止录音" value={settings.enterToStop} onChange={v=>patch("enterToStop",v)}/>}/>
      <SettingRow title="悬浮窗显示" hint="录音时显示状态悬浮窗" action={<Toggle label="悬浮窗" value={settings.overlayEnabled} onChange={v=>patch("overlayEnabled",v)}/>}/>
      <SettingRow title="实时文字预览" hint="在悬浮窗中显示临时识别文字" action={<Toggle label="实时预览" value={settings.livePreview} onChange={v=>patch("livePreview",v)}/>}/>
      <SectionLabel index="04">通用</SectionLabel><SettingRow title="外观" hint="切换后立即生效" action={<select value={settings.appearance} onChange={e=>patch("appearance",e.target.value as AppSettings['appearance'])}><option value="System">跟随系统</option><option value="Light">亮色</option><option value="Dark">暗色</option></select>}/>
      <SettingRow title="麦克风来源" hint="开发板不可用时在录音前回退电脑麦克风" action={<select value={settings.microphoneSource} onChange={e=>patch("microphoneSource",e.target.value as AppSettings['microphoneSource'])}><option value="KeyboardPreferred">键盘优先</option><option value="Computer">电脑麦克风</option></select>}/>
    </div>
  </div><div className="save-bar"><span>{status}</span><Button onClick={()=>setSettings(runtime.settings)}>撤销更改</Button><Button kind="primary" onClick={save}>保存设置</Button></div></div>;
}
