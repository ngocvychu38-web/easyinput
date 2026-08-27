import { Bot, ExternalLink, Eye, EyeOff, LockKeyhole, Server, Waves } from "lucide-react";
import { useEffect, useState } from "react";
import { getArkModelConfig, getDoubaoSpeechConfig, saveArkModelConfig, saveDoubaoSpeechConfig, testArkConnection, testDoubaoConnection } from "../api";
import { DEFAULT_ARK_CONFIG, DEFAULT_DOUBAO_CONFIG, type ArkModelConfig, type DoubaoSpeechConfig } from "../types";
import { Button, SectionLabel, SettingRow, Toggle } from "../components/Ui";
import { formatOperationError, withTimeout } from "../async";

export function SpeechConfigPage() {
  const [config,setConfig]=useState<DoubaoSpeechConfig>(DEFAULT_DOUBAO_CONFIG);const [token,setToken]=useState("");const [showToken,setShowToken]=useState(false);const [status,setStatus]=useState("");const [testing,setTesting]=useState(false);
  const [ark,setArk]=useState<ArkModelConfig>(DEFAULT_ARK_CONFIG);const [arkKey,setArkKey]=useState("");const [showArkKey,setShowArkKey]=useState(false);const [arkStatus,setArkStatus]=useState("");const [testingArk,setTestingArk]=useState(false);
  useEffect(()=>{void Promise.all([getDoubaoSpeechConfig().then(setConfig),getArkModelConfig().then(setArk)]).catch(e=>setStatus(String(e)))},[]);
  const patch=<K extends keyof DoubaoSpeechConfig>(key:K,value:DoubaoSpeechConfig[K])=>setConfig({...config,[key]:value});
  const save=async()=>{setStatus("正在保存…");try{const r=await withTimeout(saveDoubaoSpeechConfig(config,token),15_000,"保存配置超时，请检查系统是否正在等待操作。");if(r.ok&&r.data){setConfig(r.data);setToken("");setStatus(token?"配置已保存，Access Token 已写入 macOS 钥匙串。":"配置已保存，未读取或修改钥匙串中的 Access Token。")}else setStatus(r.message??"保存失败")}catch(reason){setStatus(`保存失败：${formatOperationError(reason)}`)}};
  const test=async()=>{
    if(testing)return;
    if(!config.appKey.trim()){setStatus("请先填写 App Key。");return}
    if(!token.trim()&&!config.accessTokenSaved){setStatus("请先填写 Access Token，或保存一份有效令牌。");return}
    setTesting(true);setStatus("正在连接豆包语音服务…");
    try{const r=await withTimeout(testDoubaoConnection(config,token),15_000,"连接测试超过 15 秒，已停止等待。请检查网络、代理和资源 ID。");setStatus(r.ok&&r.data?`连接成功，握手耗时 ${r.data.latencyMs} ms。${r.data.logId?` LogID：${r.data.logId}`:""}`:r.message??"连接失败")}
    catch(reason){setStatus(`连接失败：${formatOperationError(reason)}`)}
    finally{setTesting(false)}
  };
  const patchArk=<K extends keyof ArkModelConfig>(key:K,value:ArkModelConfig[K])=>setArk({...ark,[key]:value});
  const saveArk=async()=>{setArkStatus("正在保存…");try{const result=await withTimeout(saveArkModelConfig(ark,arkKey),15_000,"保存方舟配置超时");if(result.ok&&result.data){setArk(result.data);setArkKey("");setArkStatus(arkKey?"方舟配置已保存，API Key 已写入 macOS 钥匙串。":"方舟配置已保存，未修改钥匙串中的 API Key。")}else setArkStatus(result.message??"保存失败")}catch(reason){setArkStatus(`保存失败：${formatOperationError(reason)}`)}};
  const toggleArk=async(enabled:boolean)=>{const previous=ark;const next={...ark,enabled};setArk(next);setArkStatus(enabled?"正在启用语音编辑…":"正在关闭语音编辑…");try{const result=await withTimeout(saveArkModelConfig(next,arkKey),15_000,"保存启用状态超时");if(result.ok&&result.data){setArk(result.data);if(arkKey)setArkKey("");setArkStatus(enabled?"语音编辑已启用并保存。":"语音编辑已关闭并保存。")}else{setArk(previous);setArkStatus(result.message??"保存启用状态失败")}}catch(reason){setArk(previous);setArkStatus(`保存启用状态失败：${formatOperationError(reason)}`)}};
  const testArk=async()=>{if(testingArk)return;if(!ark.model.trim()){setArkStatus("请填写方舟模型 ID。");return}if(!arkKey.trim()&&!ark.apiKeySaved){setArkStatus("请填写并保存方舟 API Key。");return}setTestingArk(true);setArkStatus("正在调用火山方舟模型…");try{const result=await withTimeout(testArkConnection({...ark,enabled:true},arkKey),65_000,"方舟模型测试超过 65 秒");setArkStatus(result.ok&&result.data?`连接成功，模型 ${result.data.model}，耗时 ${result.data.latencyMs} ms。`:result.message??"连接失败")}catch(reason){setArkStatus(`连接失败：${formatOperationError(reason)}`)}finally{setTestingArk(false)}};
  return <div className="page speech-config-page"><div className="speech-config-title"><div><SectionLabel index="01">语音识别服务</SectionLabel><h1>豆包语音识别</h1><p>使用火山引擎大模型流式语音识别，将 16 kHz 单声道 PCM 实时转换为文字。</p></div><div className="provider-badge"><Waves/><span><b>豆包语音</b><small>大模型流式识别</small></span></div></div>
    <div className="speech-config-grid"><section><SectionLabel index="02">服务与鉴权</SectionLabel>
      <SettingRow title="启用豆包语音识别" hint="关闭后不会向云端发送音频" action={<Toggle label="启用豆包语音" value={config.enabled} onChange={v=>patch("enabled",v)}/>}/>
      <label>App Key<span>来自豆包语音控制台的 APP ID / App Key</span><input value={config.appKey} onChange={e=>patch("appKey",e.target.value.trim())} placeholder="请输入 App Key" autoComplete="off"/></label>
      <label>Access Token<span>只存储在 macOS 钥匙串，不写入 config.json</span><div className="secret-input"><input type={showToken?"text":"password"} value={token} onChange={e=>setToken(e.target.value)} placeholder={config.accessTokenSaved?"已安全保存；留空表示不修改":"请输入 Access Token"} autoComplete="new-password"/><button onClick={()=>setShowToken(!showToken)} aria-label="显示或隐藏令牌">{showToken?<EyeOff/>:<Eye/>}</button></div></label>
      <label>资源 ID<span>必须与控制台实际开通的计费方式一致</span><select value={config.resourceId} onChange={e=>patch("resourceId",e.target.value)}><option value="volc.bigasr.sauc.duration">2.0 小时版（推荐）</option><option value="volc.bigasr.sauc.concurrent">2.0 并发版</option><option value="volc.seedasr.sauc.duration">SeedASR 小时版（仅限已授权账号）</option><option value="volc.seedasr.sauc.concurrent">SeedASR 并发版（仅限已授权账号）</option></select></label>
      <label>服务地址<span>为避免凭据泄露，当前版本固定使用官方 WSS 地址</span><div className="locked-input"><Server/><input value={config.endpoint} readOnly/><LockKeyhole/></div></label>
    </section><section><SectionLabel index="03">识别参数</SectionLabel>
      <SettingRow title="数字与日期规范化" hint="把口语数字转换为规范文字（ITN）" action={<Toggle label="数字规范化" value={config.enableItn} onChange={v=>patch("enableItn",v)}/>}/>
      <SettingRow title="自动标点" hint="在识别结果中自动补充标点" action={<Toggle label="自动标点" value={config.enablePunc} onChange={v=>patch("enablePunc",v)}/>}/>
      <SettingRow title="分句与实时结果" hint="返回 utterances，供悬浮窗实时预览" action={<Toggle label="分句结果" value={config.showUtterances} onChange={v=>patch("showUtterances",v)}/>}/>
      <label>识别语言<select value={config.language} onChange={e=>patch("language",e.target.value)}><option value="zh-CN">中文普通话（支持中英混说）</option></select></label>
      <div className="audio-contract"><b>音频传输格式</b><dl><div><dt>采样率</dt><dd>16 kHz</dd></div><div><dt>声道</dt><dd>单声道</dd></div><div><dt>编码</dt><dd>PCM S16LE</dd></div><div><dt>分片</dt><dd>约 200 ms</dd></div></dl></div>
      <div className="privacy-notice"><LockKeyhole/><p><b>数据说明</b><br/>启用后，录音会发送到火山引擎进行识别。本机历史、设置和 Access Token 不会同步到 EasyInput 自有服务器。</p></div>
    </section></div>
    <section className="soft-panel ark-model-panel"><div className="speech-config-title"><div><SectionLabel index="04">语音编辑模型</SectionLabel><h2>火山方舟文本模型</h2><p>语音编辑会把语音转写作为问题；存在选中文本时将其作为上下文，并用模型回答替换选区。</p></div><div className="provider-badge"><Bot/><span><b>火山方舟</b><small>Responses API</small></span></div></div>
      <SettingRow title="启用语音编辑" hint="开关变更会立即保存；关闭后语音输入仍可使用" action={<Toggle label="启用方舟模型" value={ark.enabled} onChange={value=>void toggleArk(value)}/>}/>
      <div className="speech-config-grid"><label>模型 ID<span>填写方舟控制台中已开通的模型 ID 或推理接入点 ID</span><input value={ark.model} onChange={event=>patchArk("model",event.target.value.trim())} placeholder="doubao-seed-2-0-lite-260215" autoComplete="off"/></label>
      <label>Ark API Key<span>与语音识别 Access Token 不同；只存储在 macOS 钥匙串</span><div className="secret-input"><input type={showArkKey?"text":"password"} value={arkKey} onChange={event=>setArkKey(event.target.value)} placeholder={ark.apiKeySaved?"已安全保存；留空表示不修改":"请输入方舟 API Key"} autoComplete="new-password"/><button onClick={()=>setShowArkKey(!showArkKey)} aria-label="显示或隐藏方舟 API Key">{showArkKey?<EyeOff/>:<Eye/>}</button></div></label></div>
      <label>服务地址<span>固定使用火山方舟官方 Responses API，避免 API Key 被发送到其他地址</span><div className="locked-input"><Server/><input value={ark.endpoint} readOnly/><LockKeyhole/></div></label>
      <div className="save-bar"><a href="https://www.volcengine.com/docs/82379/1795150" target="_blank" rel="noreferrer">打开方舟官方文档 <ExternalLink/></a><span>{arkStatus}</span><Button onClick={testArk} disabled={testingArk||!ark.model}>{testingArk?"测试中…":"测试模型"}</Button><Button kind="primary" onClick={saveArk}>保存方舟配置</Button></div>
    </section>
    <div className="save-bar"><a href="https://docs.volcengine.com/docs/6561/1354869?lang=zh" target="_blank" rel="noreferrer">打开官方文档 <ExternalLink/></a><span>{status}</span><Button onClick={test} disabled={testing||!config.appKey}>{testing?"测试中…":"测试连接"}</Button><Button kind="primary" onClick={save}>保存配置</Button></div>
  </div>;
}
