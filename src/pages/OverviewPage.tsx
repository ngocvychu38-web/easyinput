import { useEffect, useMemo, useState } from "react";
import { getActivityCalendar } from "../api";
import type { ActivityDay, RuntimeSnapshot } from "../types";
import { SectionLabel } from "../components/Ui";

const trend = [12, 10, 15, 44, 63, 38, 13, 9, 12, 26, 31, 20, 12, 14];
export function OverviewPage({ runtime,navigate }: { runtime: RuntimeSnapshot; refresh(): Promise<void>;navigate(page:"dictionary"|"settings"):void }) {
  const [view, setView] = useState<"overview"|"trend">("overview");
  const chars = runtime.todayChars;
  const seconds = Math.round(runtime.todayDurationMs / 1000);
  const speed = seconds > 0 ? Math.round(chars / seconds * 60) : 0;
  const points = trend.map((v, i) => `${i * (640/(trend.length-1))},${90-v}`).join(" ");
  return <div className="page overview-page">
    <div className="page-toolbar"><SectionLabel index="01">输入概览</SectionLabel><div className="segmented"><button className={view === "overview" ? "active" : ""} onClick={() => setView("overview")}>概览</button><button className={view === "trend" ? "active" : ""} onClick={() => setView("trend")}>趋势</button><button>今天⌄</button></div></div>
    {view === "overview" ? <>
      <section className="hero-metric">
        <div className="metric-main"><div><span className="number">{chars}</span><span className="unit">字</span></div><div className="metric-sub"><span><b>{seconds}</b> 秒<small>语音时长</small></span><span><b>{speed}</b><small>速度 / 字/分</small></span><span><b>{Math.round(seconds * .6)}</b> 秒<small>预计节省</small></span></div></div>
        <div className="wave-chart" aria-label="当天输入波形">{Array.from({length: 65},(_,i)=><i key={i} style={{height:`${8 + Math.sin(i/4)**2*24 + (i>15&&i<25?30:0)}px`}} />)}<div className="axis"><span>07:00</span><span>09:00</span><span>11:00</span><span>今天</span></div></div>
      </section>
      <section className="overview-grid">
        <div><SectionLabel index="02">输入趋势</SectionLabel><svg className="line-chart" viewBox="0 0 640 100" preserveAspectRatio="none"><path d="M0 90H640" /><polyline points={points} /></svg><div className="chart-labels"><span>13天前</span><span>7天前</span><span>今天</span></div></div>
        <div><SectionLabel index="03">效率</SectionLabel><dl className="stats-list"><div><dt>本周</dt><dd>{chars.toLocaleString()}<small> 字</small></dd></div><div><dt>平均</dt><dd>{Math.round(chars/7)}<small> 字/日</small></dd></div><div><dt>和7日均值相比</dt><dd>0%</dd></div><div><dt>预计节省</dt><dd>{Math.round(seconds*.6)}<small> 秒</small></dd></div></dl></div>
        <div><SectionLabel index="04">近 7 天</SectionLabel><h2>今天输入</h2><p>近 7 天累计 {chars.toLocaleString()} 字，平均每天 {Math.round(chars/7)} 字。</p><div className="two-stats"><span><b>1</b><small>活跃天数</small></span><span><b>{speed}</b><small>平均速度</small></span></div></div>
      </section>
      <OverviewExtras navigate={navigate}/>
    </> : <TrendView points={points} />}
  </div>;
}

function OverviewExtras({navigate}:{navigate(page:"dictionary"|"settings"):void}){
  const now=new Date();
  const [month,setMonth]=useState(()=>`${now.getFullYear()}-${String(now.getMonth()+1).padStart(2,"0")}`);
  const [activity,setActivity]=useState<ActivityDay[]>([]);
  const [loading,setLoading]=useState(true);
  const [error,setError]=useState("");
  const [year,monthNumber]=month.split("-").map(Number);
  useEffect(()=>{setLoading(true);setError("");void getActivityCalendar(year,monthNumber).then(setActivity).catch(reason=>setError(reason instanceof Error?reason.message:String(reason))).finally(()=>setLoading(false))},[year,monthNumber]);
  const months=useMemo(()=>Array.from({length:12},(_,index)=>{const date=new Date(now.getFullYear(),now.getMonth()-index,1);return{value:`${date.getFullYear()}-${String(date.getMonth()+1).padStart(2,"0")}`,label:`${date.getFullYear()}年${date.getMonth()+1}月`}}),[]);
  const active=useMemo(()=>new Map(activity.map(day=>[day.day,day])),[activity]);
  const totalDays=new Date(year,monthNumber,0).getDate();
  const offset=(new Date(year,monthNumber-1,1).getDay()+6)%7;
  const cells:Array<number|null>=[...Array.from({length:offset},()=>null),...Array.from({length:totalDays},(_,index)=>index+1)];while(cells.length%7)cells.push(null);
  const maxChars=Math.max(1,...activity.map(day=>day.charCount));
  const activeDays=[...active.keys()].sort((left,right)=>left-right);let longest=0,current=0,previous=0;for(const day of activeDays){current=day===previous+1?current+1:1;longest=Math.max(longest,current);previous=day}
  const isCurrentMonth=year===now.getFullYear()&&monthNumber===now.getMonth()+1;
  return <section className="overview-extras">
    <div className="activity-panel">
      <div className="extra-heading"><SectionLabel index="05">活跃日历</SectionLabel><select value={month} onChange={event=>setMonth(event.target.value)} aria-label="选择月份">{months.map(item=><option key={item.value} value={item.value}>{item.label}</option>)}</select></div>
      <div className="calendar-weekdays">{["一","二","三","四","五","六","日"].map(day=><span key={day}>{day}</span>)}</div>
      <div className={`activity-calendar ${loading?"loading-calendar":""}`}>{cells.map((day,index)=>{if(!day)return <i key={`blank-${index}`}/>;const data=active.get(day);const level=data?Math.max(1,Math.ceil(data.charCount/maxChars*3)):0;const today=isCurrentMonth&&day===now.getDate();return <span key={day} className={`${level?`level-${level}`:""} ${today?"today":""}`} title={data?`${monthNumber}月${day}日 · ${data.charCount} 字 · ${Math.round(data.durationMs/1000)} 秒`:`${monthNumber}月${day}日 · 暂无输入`}>{day}</span>})}</div>
      {error?<p className="calendar-error">读取活跃记录失败：{error}</p>:<p className="calendar-summary">本月活跃 <b>{activity.length}</b> 天　·　连续 <b>{longest}</b> 天</p>}
    </div>
    <div className="quick-panel"><SectionLabel index="06">快捷入口</SectionLabel><div className="quick-links"><button onClick={()=>navigate("dictionary")}><span>词库管理<small>维护热词与替换规则</small></span><b>打开</b></button><button onClick={()=>navigate("settings")}><span>设置中心<small>快捷键、输入与外观</small></span><b>打开</b></button></div></div>
  </section>
}

function TrendView({ points }: { points: string }) {
  return <div className="trend-view"><h1>输入趋势</h1><p>最近 14 天的语音输入活动</p><svg viewBox="0 0 640 150" preserveAspectRatio="none"><path d="M0 140H640M0 95H640M0 50H640"/><polyline points={points.split(" ").map(p=>{const [x,y]=p.split(",");return `${x},${Number(y)+40}`}).join(" ")} /></svg><div className="trend-summary"><span><b>0</b><small>日均字数</small></span><span><b>1</b><small>活跃天数</small></span><span><b>0 秒</b><small>节省时间</small></span></div></div>;
}
