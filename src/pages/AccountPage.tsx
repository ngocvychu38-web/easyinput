import { useState } from "react";
import { login, logout } from "../api";
import { Button, SectionLabel } from "../components/Ui";

export function AccountPage() {
  const [user,setUser]=useState<{name:string;email:string}|null>(null);const [form,setForm]=useState({email:"",password:""});const [message,setMessage]=useState("");
  const signIn=async()=>{if(!form.email||!form.password){setMessage("请输入邮箱和密码");return}const r=await login(form.email,form.password);if(r.ok)setUser({name:"EasyInput 用户",email:form.email});else setMessage(r.message??"登录失败")};
  if(!user) return <div className="page account-page login-view"><section><SectionLabel index="01">账户</SectionLabel><h1>登录 EasyInput</h1><p>登录后可使用云端语音识别，并同步你的热词。</p><label>邮箱<input type="email" value={form.email} onChange={e=>setForm({...form,email:e.target.value})} placeholder="name@example.com"/></label><label>密码<input type="password" value={form.password} onChange={e=>setForm({...form,password:e.target.value})}/></label>{message&&<p className="form-error">{message}</p>}<Button kind="primary" onClick={signIn}>登录</Button><button className="text-button">注册新账户</button><button className="text-button">忘记密码</button></section></div>;
  return <div className="page account-page"><div className="page-toolbar"><div><SectionLabel index="01">账户</SectionLabel><p>管理账号与登录状态</p></div><Button onClick={async()=>{await logout();setUser(null)}}>退出登录</Button></div><div className="account-grid"><dl><div><dt>当前用户</dt><dd>{user.name}</dd></div><div><dt>邮箱</dt><dd>{user.email}</dd></div><div><dt>邮箱状态</dt><dd>已验证</dd></div><div><dt>登录方式</dt><dd><span className="pill">邮箱密码</span></dd></div></dl><section><SectionLabel index="02">使用方案</SectionLabel><p>当前版本免费开放使用</p><span className="pill">免费使用中</span></section></div></div>;
}
