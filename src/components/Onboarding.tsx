import { Check, Keyboard, Mic, ShieldCheck, Sparkles } from "lucide-react";
import { useState } from "react";
import { Button } from "./Ui";

const steps = [
  { icon: Sparkles, title: "欢迎使用 EasyInput", copy: "在任何可编辑应用中，用语音跟上你的想法。此版本为 Intel Mac 原生应用。" },
  { icon: Keyboard, title: "记住两个快捷键", copy: "右 Command 用于语音输入，右 Option 用于对选中文字发出编辑指令。默认按住说话，松开结束。" },
  { icon: ShieldCheck, title: "授予本机权限", copy: "首次使用时，macOS 会请求麦克风、辅助功能、输入监控、蓝牙和本地网络权限。拒绝后可在系统设置的隐私与安全性中重新开启。" },
  { icon: Mic, title: "准备开始", copy: "先把光标放进目标输入框，再按住右 Command 说话。开发板麦克风不可用时，会在录音开始前使用电脑麦克风。" }
];
export function Onboarding({onComplete}:{onComplete():void}) { const [step,setStep]=useState(0);const current=steps[step];const Icon=current.icon;return <div className="onboarding-backdrop"><section className="onboarding-card"><div className="onboarding-mark"><Icon size={28}/></div><div className="onboarding-count">{String(step+1).padStart(2,"0")} / 04</div><h1>{current.title}</h1><p>{current.copy}</p>{step===2&&<div className="permission-list"><span><Check/>麦克风</span><span><Check/>辅助功能与输入监控</span><span><Check/>蓝牙与本地网络</span></div>}<div className="onboarding-actions">{step>0&&<Button onClick={()=>setStep(step-1)}>上一步</Button>}<Button kind="primary" onClick={()=>step===steps.length-1?onComplete():setStep(step+1)}>{step===steps.length-1?"进入 EasyInput":"继续"}</Button></div><button className="onboarding-skip" onClick={onComplete}>跳过引导</button></section></div> }
