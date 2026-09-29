export type VoiceModule = "live" | "english" | "cockpit" | "cube" | "mcd";
export type VoiceNavigation = { action: "open" | "close"; module: VoiceModule };

const phrases: [RegExp, VoiceModule][] = [
  [/^(?:三阶)?魔方/, "cube"],
  [/^(?:直播工作室|直播间|直播)/, "live"],
  [/^(?:英语学习|英语)/, "english"],
  [/^(?:训练营驾驶舱|驾驶舱|训练营)/, "cockpit"],
  [/^(?:麦当劳点餐|麦当劳|点餐)/, "mcd"],
];

function clauses(text: string) {
  return text.split(/[，。！？；、,.!?;:\n]+/).map(part => part.replace(/^[\s]*(?:(?:嗯+|啊+|诶+|呃+|那个|然后|好的?|请问|我想|我说|就是)+)/, "").replace(/[\s“”"'‘’]/g, "")).filter(Boolean);
}

function findTarget(remainder: string): VoiceModule | undefined {
  for (const [pattern, module] of phrases) {
    const match = remainder.match(pattern);
    if (!match) continue;
    let suffix = remainder.slice(match[0].length);
    while (suffix) {
      const next = suffix.replace(/^(?:吧|啊|呀|呢|一下|页面|模块|工作室|功能)/, "");
      if (next === suffix) break;
      suffix = next;
    }
    if (!suffix) return module;
  }
  return undefined;
}

export function voiceNavigationFromTranscript(text: string): VoiceNavigation | undefined {
  for (const clause of clauses(text)) {
    for (const [verb, action] of [["打开", "open"], ["开启", "open"], ["启动", "open"], ["进入", "open"], ["切换到", "open"], ["去", "open"], ["关闭", "close"], ["关掉", "close"], ["退出", "close"], ["返回", "close"], ["隐藏", "close"]] as const) {
      let remainder = clause.startsWith(verb) ? clause.slice(verb.length) : "";
      if (action === "close" && clause.startsWith(verb)) remainder = remainder.replace(/^(?:一下|掉|这个|当前)/, "");
      if (action === "close" && !clause.startsWith(verb)) continue;
      if (action === "open" && !clause.startsWith(verb)) continue;
      const module = findTarget(remainder);
      if (module) return { action, module };
    }
    if (clause.startsWith("不要")) {
      const module = findTarget(clause.slice(2));
      if (module) return { action: "close", module };
    }
  }
  return undefined;
}
