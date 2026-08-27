import { useEffect, useState } from "react";
import { Copy, Download, Trash2 } from "lucide-react";
import { deleteHistory, getHistoryPage } from "../api";
import type { HistoryEntry } from "../types";
import { Button, SectionLabel } from "../components/Ui";

export function HistoryPage() {
  const [items, setItems] = useState<HistoryEntry[]>([]); const [loading, setLoading] = useState(true);
  const load = async () => { setLoading(true); try { setItems(await getHistoryPage(undefined, 50)); } finally { setLoading(false); } };
  useEffect(() => { void load(); }, []);
  const remove = async (id: number) => { if (!confirm("确定删除这条记录吗？")) return; const r=await deleteHistory(id); if(r.ok) setItems(v=>v.filter(x=>x.id!==id)); };
  const exportData = () => { const blob = new Blob([items.map(x=>`${x.createdAt}\n${x.text}`).join("\n\n")], {type:"text/plain"}); const a=document.createElement("a");a.href=URL.createObjectURL(blob);a.download="EasyInput-历史.txt";a.click();URL.revokeObjectURL(a.href); };
  return <div className="page history-page">
    <div className="page-toolbar"><div><SectionLabel index="01">历史记录</SectionLabel><p>你的语音输入记录只保存在这台电脑。</p></div><div className="actions"><Button onClick={exportData}><Download size={15}/> 导出</Button><Button kind="danger" disabled={!items.length}><Trash2 size={15}/> 清空</Button></div></div>
    <div className="history-list">{loading ? <div className="empty">正在读取历史…</div> : items.length === 0 ? <div className="empty"><span>暂无历史记录</span><p>完成一次语音输入后，内容会出现在这里。</p></div> : items.map(item=><article key={item.id}>
      <div className="history-meta"><time>{new Date(item.createdAt).toLocaleString("zh-CN")}</time><span>{item.charCount} 字 · {Math.round(item.durationMs/1000)} 秒 · {item.source === "Keyboard" ? "键盘麦克风" : "电脑麦克风"}</span></div>
      <p>{item.text}</p><div className="history-actions"><button onClick={()=>navigator.clipboard.writeText(item.text)}><Copy size={14}/>复制</button><button onClick={()=>remove(item.id)}><Trash2 size={14}/>删除</button></div>
    </article>)}</div>
  </div>;
}
