import { CheckCircle2, ExternalLink, Eye, EyeOff, LockKeyhole, QrCode, ShieldCheck, Utensils } from "lucide-react";
import { useEffect, useState } from "react";
import { getMcdMcpConfig, saveMcdMcpConfig, testMcdMcpConnection } from "../api";
import { formatOperationError, withTimeout } from "../async";
import { Button, SectionLabel, SettingRow, Toggle } from "../components/Ui";
import { DEFAULT_MCD_MCP_CONFIG, type McdMcpConfig } from "../types";

export function McdPage() {
  const [config,setConfig]=useState<McdMcpConfig>(DEFAULT_MCD_MCP_CONFIG);
  const [token,setToken]=useState("");
  const [showToken,setShowToken]=useState(false);
  const [status,setStatus]=useState("");
  const [testing,setTesting]=useState(false);
  useEffect(()=>{void getMcdMcpConfig().then(setConfig).catch(error=>setStatus(String(error)))},[]);
  const save=async()=>{
    setStatus("正在保存…");
    try{
      const result=await withTimeout(saveMcdMcpConfig(config,token),15_000,"保存 MCP 配置超时");
      if(!result.ok||!result.data){setStatus(result.message??"保存失败");return}
      setConfig(result.data);if(token)setToken("");setStatus(token?"配置已保存，Token 已写入 macOS 钥匙串。":"配置已保存，未修改钥匙串中的 Token。");
    }catch(error){setStatus(`保存失败：${formatOperationError(error)}`)}
  };
  const test=async()=>{
    if(testing)return;
    if(!token.trim()&&!config.tokenSaved){setStatus("请先填写麦当劳 MCP Token。");return}
    setTesting(true);setStatus("正在初始化 MCP 并读取工具清单…");
    try{
      const result=await withTimeout(testMcdMcpConnection({...config,enabled:true},token),35_000,"MCP 连接测试超过 35 秒");
      if(!result.ok||!result.data){setStatus(result.message??"连接失败");return}
      setStatus(`连接成功：${result.data.serverName} · ${result.data.protocolVersion} · ${result.data.tools.length} 个工具 · ${result.data.latencyMs} ms`);
    }catch(error){setStatus(`连接失败：${formatOperationError(error)}`)}finally{setTesting(false)}
  };
  const toggle=async(enabled:boolean)=>{
    const previous=config;const next={...config,enabled};setConfig(next);
    try{const result=await saveMcdMcpConfig(next,token);if(result.ok&&result.data){setConfig(result.data);if(token)setToken("");setStatus(enabled?"麦当劳语音点餐已启用。":"麦当劳语音点餐已关闭。")}else{setConfig(previous);setStatus(result.message??"保存失败")}}catch(error){setConfig(previous);setStatus(formatOperationError(error))}
  };
  return <div className="page mcd-page">
    <header className="mcd-head"><div><SectionLabel index="01">MCP 语音点餐</SectionLabel><h1>麦当劳外送</h1><p>豆包端到端语音模型调用本地高层 Function，再由 EasyInput 串行调用麦当劳 MCP。支付环节只在浏览器展示二维码。</p></div><div className="mcd-badge"><Utensils/><span><b>McDonald&apos;s MCP</b><small>Streamable HTTP</small></span></div></header>
    <div className="mcd-grid"><section><SectionLabel index="02">连接与鉴权</SectionLabel>
      <SettingRow title="启用语音点餐" hint="启用后，下次实时通话会加载 9 个麦当劳点餐工具" action={<Toggle label="启用麦当劳 MCP" value={config.enabled} onChange={value=>void toggle(value)}/>}/>
      <label>MCP Token<span>从麦当劳 MCP 控制台获取；只存储在 macOS 钥匙串</span><div className="mcd-secret"><input type={showToken?"text":"password"} value={token} onChange={event=>setToken(event.target.value)} placeholder={config.tokenSaved?"已安全保存；留空表示不修改":"请输入 MCP Token"} autoComplete="new-password"/><button onClick={()=>setShowToken(!showToken)} aria-label="显示或隐藏 Token">{showToken?<EyeOff/>:<Eye/>}</button></div></label>
      <label>官方服务地址<span>固定地址，防止 Token 被发送到第三方服务器</span><div className="mcd-locked"><LockKeyhole/><input value={config.endpoint} readOnly/></div></label>
      <label>MCP 协议版本<span>初始化及后续会话请求使用此版本</span><div className="mcd-locked"><LockKeyhole/><input value={config.protocolVersion} readOnly/></div></label>
    </section><section><SectionLabel index="03">MVP 边界</SectionLabel>
      <div className="mcd-flow">{["读取已保存地址与可配送门店","查询实时菜单并选择简单单品","计算最终价格并语音复述","当前轮明确说“确认下单”才创建订单","浏览器展示官方支付二维码","支付后语音查询订单状态"].map((item,index)=><div key={item}><span>{String(index+1).padStart(2,"0")}</span><CheckCircle2/><p>{item}</p></div>)}</div>
      <div className="mcd-safety"><ShieldCheck/><p><b>首版安全约束</b><br/>模型看不到真实地址 ID、门店编码、商品编码和支付链接；内部调用按会话串行执行。套餐和特制餐品可采用服务端默认配置，暂不支持语音逐项修改配置或自动选择优惠券。</p></div>
      <div className="mcd-payment"><QrCode/><p><b>支付不在语音链路内完成</b><br/>订单创建成功后，EasyInput 自动启动本机一次性二维码页。用户必须亲自扫码确认支付。</p></div>
    </section></div>
    <div className="save-bar"><a href="https://open.mcd.cn/mcp" target="_blank" rel="noreferrer">麦当劳 MCP 控制台 <ExternalLink/></a><span>{status}</span><Button onClick={test} disabled={testing}>{testing?"测试中…":"测试连接"}</Button><Button kind="primary" onClick={save}>保存配置</Button></div>
  </div>;
}
