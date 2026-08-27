import type { ButtonHTMLAttributes, PropsWithChildren, ReactNode } from "react";

export function SectionLabel({ index, children }: PropsWithChildren<{ index: string }>) {
  return <div className="section-label"><span>{index}</span>{children}</div>;
}
export function Button({ children, kind = "secondary", ...props }: PropsWithChildren<ButtonHTMLAttributes<HTMLButtonElement> & { kind?: "primary"|"secondary"|"ghost"|"danger" }>) {
  return <button {...props} className={`button ${kind} ${props.className ?? ""}`.trim()}>{children}</button>;
}
export function Toggle({ value, onChange, label }: { value: boolean; onChange(v: boolean): void; label: string }) {
  return <button role="switch" aria-checked={value} aria-label={label} onClick={() => onChange(!value)} className={`toggle ${value ? "on" : ""}`}><span /></button>;
}
export function SettingRow({ title, hint, action }: { title: string; hint?: string; action: ReactNode }) {
  return <div className="setting-row"><div><div className="setting-title">{title}</div>{hint && <div className="hint">{hint}</div>}</div><div>{action}</div></div>;
}
