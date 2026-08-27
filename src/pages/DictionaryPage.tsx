import { useEffect, useRef, useState } from "react";
import { Download, Upload } from "lucide-react";
import { open, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { exportDictionaryFile, getDictionary, importDictionaryFile, saveDictionary } from "../api";
import { Button, SectionLabel } from "../components/Ui";

export function DictionaryPage() {
  const [hotwords,setHotwords]=useState<string[]>([]);
  const [replacements,setReplacements]=useState<[string,string][]>([]);
  const [newWord,setNewWord]=useState("");
  const [loading,setLoading]=useState(true);
  const [busy,setBusy]=useState<"import"|"export"|"save">();
  const [status,setStatus]=useState("");
  const [isError,setIsError]=useState(false);
  const [autoSave,setAutoSave]=useState<"idle"|"saving"|"saved"|"error">("idle");
  const saveQueue=useRef<Promise<void>>(Promise.resolve());

  useEffect(()=>{void getDictionary().then(data=>{setHotwords(data.hotwords);setReplacements(data.replacements)}).catch(reason=>{setStatus(`读取词库失败：${reason instanceof Error?reason.message:String(reason)}`);setIsError(true)}).finally(()=>setLoading(false))},[]);
  useEffect(()=>{if(loading)return;setAutoSave("saving");const timer=window.setTimeout(()=>{const words=[...hotwords];const rules=replacements.map(rule=>[...rule]as[string,string]);saveQueue.current=saveQueue.current.then(async()=>{const result=await saveDictionary(words,rules);if(!result.ok)throw new Error(result.message??"自动保存失败")}).then(()=>setAutoSave("saved")).catch(reason=>{setAutoSave("error");setStatus(`自动保存失败：${reason instanceof Error?reason.message:String(reason)}`);setIsError(true)})},600);return()=>window.clearTimeout(timer)},[hotwords,replacements,loading]);

  const show=(message:string,error=false)=>{setStatus(message);setIsError(error)};
  const add=()=>{const word=newWord.trim();if(!word)return;if(hotwords.includes(word)){show(`“${word}”已经在词库中`,true)}else if(hotwords.length>=1000){show("热词数量不能超过 1000 个",true)}else{setHotwords([...hotwords,word]);show("已添加，点击“保存更改”写入本机")};setNewWord("")};
  const persist=async(words=hotwords,rules=replacements)=>{const result=await saveDictionary(words,rules);if(!result.ok)throw new Error(result.message??"保存失败")};
  const save=async()=>{setBusy("save");try{await persist();show("词库已保存到本机")}catch(reason){show(`保存失败：${reason instanceof Error?reason.message:String(reason)}`,true)}finally{setBusy(undefined)}};

  const importFile=async()=>{
    setBusy("import");show("");
    try{
      const path=await open({multiple:false,directory:false,title:"导入 EasyInput 词库",filters:[{name:"EasyInput 词库",extensions:["txt"]}]});
      if(!path)return;
      const result=await importDictionaryFile(path);
      if(!result.ok||!result.data)throw new Error(result.message??"导入失败");
      const existing=new Set(hotwords);const added=result.data.words.filter(word=>!existing.has(word));const skippedExisting=result.data.words.length-added.length;
      if(hotwords.length+added.length>1000)throw new Error(`合并后有 ${hotwords.length+added.length} 个热词，超过 1000 个限制`);
      const merged=[...hotwords,...added];await persist(merged,replacements);setHotwords(merged);
      const skipped=skippedExisting+result.data.duplicateLines;const details=[`新增 ${added.length} 个`,skipped?`跳过重复 ${skipped} 个`:"",result.data.blankLines?`忽略空行 ${result.data.blankLines} 行`:""].filter(Boolean).join("，");
      show(`导入并保存完成：${details}`);
    }catch(reason){show(`导入失败：${reason instanceof Error?reason.message:String(reason)}`,true)}finally{setBusy(undefined)}
  };

  const exportFile=async()=>{
    setBusy("export");show("");
    try{
      const path=await saveDialog({title:"导出 EasyInput 词库",defaultPath:"easy-input-dictionary.txt",filters:[{name:"EasyInput 词库",extensions:["txt"]}]});
      if(!path)return;
      const result=await exportDictionaryFile(path,hotwords);
      if(!result.ok||!result.data)throw new Error(result.message??"导出失败");
      const name=result.data.path.split("/").pop()??result.data.path;show(`已导出 ${result.data.count} 个热词到 ${name}`);
    }catch(reason){show(`导出失败：${reason instanceof Error?reason.message:String(reason)}`,true)}finally{setBusy(undefined)}
  };

  return <div className="page dictionary-page">
    <section><div className="dictionary-head"><SectionLabel index="01">热词 · {hotwords.length} 个</SectionLabel><div><Button onClick={importFile} disabled={loading||Boolean(busy)}><Upload size={14}/>{busy==="import"?"导入中…":"导入"}</Button><Button onClick={exportFile} disabled={loading||Boolean(busy)||hotwords.length===0}><Download size={14}/>{busy==="export"?"导出中…":"导出"}</Button><Button kind="primary" onClick={save} disabled={loading||Boolean(busy)}>{busy==="save"?"保存中…":"保存更改"}</Button></div></div><p>让语音识别更容易听对人名、产品名、项目名等专有名词。导入与导出文件采用 UTF-8 编码，每行一个热词。</p>
      {status&&<div className={`dictionary-status ${isError?"error":""}`}>{status}</div>}
      {loading?<div className="replacement-empty">正在读取本机词库…</div>:<div className="chips">{hotwords.map(word=><span key={word}>{word}<button aria-label={`移除${word}`} onClick={()=>{setHotwords(hotwords.filter(value=>value!==word));show("已移除，点击“保存更改”写入本机")}}>×</button></span>)}<label className="add-chip">＋<input value={newWord} onChange={event=>setNewWord(event.target.value)} onKeyDown={event=>{if(event.key==="Enter"){event.preventDefault();add()}}} placeholder="添加" /></label></div>}
    </section>
    <section><div className="replacement-heading"><SectionLabel index="02">替换规则 · {replacements.length} 条</SectionLabel><span className={autoSave==="error"?"error":""}>{autoSave==="saving"?"正在自动保存…":autoSave==="saved"?"已自动保存":autoSave==="error"?"自动保存失败":""}</span></div><p>豆包返回实时转写后，立即按从上到下的顺序进行替换；最终文本和历史记录同样保存替换后的内容。</p>{replacements.length===0?<div className="replacement-empty">暂无替换规则，点击下方添加</div>:replacements.map((rule,index)=><div className="replacement" key={index}><input value={rule[0]} onChange={event=>setReplacements(replacements.map((value,current)=>current===index?[event.target.value,value[1]]:value))} placeholder="识别结果"/><span>→</span><input value={rule[1]} onChange={event=>setReplacements(replacements.map((value,current)=>current===index?[value[0],event.target.value]:value))} placeholder="替换为"/><button aria-label="删除替换规则" onClick={()=>setReplacements(replacements.filter((_,current)=>current!==index))}>×</button></div>)}<button className="text-button" onClick={()=>setReplacements([...replacements,["",""]])}>＋ 添加规则</button></section>
  </div>;
}
