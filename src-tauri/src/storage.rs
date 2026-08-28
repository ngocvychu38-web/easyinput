use crate::model::{ActivityDay, AppSettings, ArkModelConfig, DoubaoSpeechConfig, HistoryEntry, KeyboardConfig, RealtimeVoiceConfig};
use crate::dictionary::DictionaryData;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::{fs, path::{Path, PathBuf}, sync::Mutex};

const CONFIG_VERSION: u32 = 4;
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all="camelCase")]
pub struct PersistedConfig { pub version: u32, pub revision: u64, pub settings: AppSettings, pub keyboard: KeyboardConfig, #[serde(default)] pub speech: DoubaoSpeechConfig, #[serde(default)] pub ark: ArkModelConfig, #[serde(default)] pub realtime_voice: RealtimeVoiceConfig }
impl Default for PersistedConfig { fn default()->Self { Self{version:CONFIG_VERSION,revision:1,settings:AppSettings::default(),keyboard:KeyboardConfig::default(),speech:DoubaoSpeechConfig::default(),ark:ArkModelConfig::default(),realtime_voice:RealtimeVoiceConfig::default()} } }

pub struct Storage { root: PathBuf, conn: Mutex<Connection> }
impl Storage {
    pub fn open(root: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&root).map_err(|e| format!("无法创建数据目录: {e}"))?;
        let conn=Connection::open(root.join("history.db")).map_err(|e|e.to_string())?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;
          CREATE TABLE IF NOT EXISTS history(id INTEGER PRIMARY KEY AUTOINCREMENT,text TEXT NOT NULL,created_at TEXT NOT NULL,duration_ms INTEGER NOT NULL,char_count INTEGER NOT NULL,source TEXT NOT NULL);
          CREATE INDEX IF NOT EXISTS idx_history_time ON history(created_at DESC,id DESC);").map_err(|e|e.to_string())?;
        Ok(Self{root,conn:Mutex::new(conn)})
    }
    pub fn root(&self)->&Path { &self.root }
    pub fn read_config(&self)->Result<PersistedConfig,String>{
        let path=self.root.join("config.json"); if !path.exists(){let cfg=PersistedConfig::default();self.write_config(&cfg)?;return Ok(cfg)}
        let bytes=fs::read(&path).map_err(|e|e.to_string())?; let mut cfg:PersistedConfig=serde_json::from_slice(&bytes).map_err(|e|format!("配置无效: {e}"))?;
        if cfg.version>CONFIG_VERSION{return Err(format!("配置版本 {} 高于当前支持的 {}，已进入保护模式",cfg.version,CONFIG_VERSION))}
        if cfg.version<2{
            cfg.speech.resource_id=match cfg.speech.resource_id.as_str(){
                "volc.seedasr.sauc.duration"=>"volc.bigasr.sauc.duration".into(),
                "volc.seedasr.sauc.concurrent"=>"volc.bigasr.sauc.concurrent".into(),
                _=>cfg.speech.resource_id,
            };
            cfg.version=2;cfg.revision+=1;
        }
        if cfg.version<4{cfg.version=4;cfg.revision+=1;self.write_config(&cfg)?;}
        Ok(cfg)
    }
    pub fn write_config(&self,cfg:&PersistedConfig)->Result<(),String>{
        let path=self.root.join("config.json"); let tmp=self.root.join("config.json.tmp");
        let bytes=serde_json::to_vec_pretty(cfg).map_err(|e|e.to_string())?;fs::write(&tmp,bytes).map_err(|e|e.to_string())?;fs::rename(tmp,path).map_err(|e|e.to_string())
    }
    pub fn backup_config(&self)->Result<Option<PathBuf>,String>{let src=self.root.join("config.json");if !src.exists(){return Ok(None)}let dst=self.root.join(format!("config.backup-{}.json",chrono::Utc::now().format("%Y%m%d-%H%M%S")));fs::copy(&src,&dst).map_err(|e|e.to_string())?;Ok(Some(dst))}
    pub fn history_page(&self,cursor:Option<i64>,limit:u32)->Result<Vec<HistoryEntry>,String>{
        let conn=self.conn.lock().map_err(|_|"历史数据库锁已损坏")?;let limit=limit.clamp(1,100);
        let mut stmt=conn.prepare("SELECT id,text,created_at,duration_ms,char_count,source FROM history WHERE (?1 IS NULL OR id < ?1) ORDER BY id DESC LIMIT ?2").map_err(|e|e.to_string())?;
        let rows=stmt.query_map(params![cursor,limit],|r|Ok(HistoryEntry{id:r.get(0)?,text:r.get(1)?,created_at:r.get(2)?,duration_ms:r.get(3)?,char_count:r.get(4)?,source:r.get(5)?})).map_err(|e|e.to_string())?;
        rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())
    }
    pub fn add_history(&self,text:&str,duration_ms:u64,source:&str)->Result<i64,String>{if text.trim().is_empty(){return Err("空文本不能写入历史".into())}let conn=self.conn.lock().map_err(|_|"历史数据库锁已损坏")?;conn.execute("INSERT INTO history(text,created_at,duration_ms,char_count,source) VALUES(?1,?2,?3,?4,?5)",params![text,chrono::Utc::now().to_rfc3339(),duration_ms,text.chars().count() as u64,source]).map_err(|e|e.to_string())?;Ok(conn.last_insert_rowid())}
    pub fn delete_history(&self,id:i64)->Result<(),String>{self.conn.lock().map_err(|_|"历史数据库锁已损坏")?.execute("DELETE FROM history WHERE id=?1",params![id]).map_err(|e|e.to_string())?;Ok(())}
    pub fn today_stats(&self)->Result<(u64,u64),String>{let conn=self.conn.lock().map_err(|_|"历史数据库锁已损坏")?;conn.query_row("SELECT COALESCE(SUM(char_count),0),COALESCE(SUM(duration_ms),0) FROM history WHERE date(created_at,'localtime')=date('now','localtime')",[],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|e|e.to_string())}
    pub fn activity_month(&self,year:i32,month:u32)->Result<Vec<ActivityDay>,String>{if !(2000..=2100).contains(&year)||!(1..=12).contains(&month){return Err("年月参数无效".into())}let key=format!("{year:04}-{month:02}");let conn=self.conn.lock().map_err(|_|"历史数据库锁已损坏")?;let mut stmt=conn.prepare("SELECT CAST(strftime('%d',datetime(created_at),'localtime') AS INTEGER),COALESCE(SUM(char_count),0),COALESCE(SUM(duration_ms),0) FROM history WHERE strftime('%Y-%m',datetime(created_at),'localtime')=?1 GROUP BY 1 ORDER BY 1").map_err(|e|e.to_string())?;let rows=stmt.query_map(params![key],|row|Ok(ActivityDay{day:row.get(0)?,char_count:row.get(1)?,duration_ms:row.get(2)?})).map_err(|e|e.to_string())?;rows.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())}
    pub fn read_dictionary(&self)->Result<DictionaryData,String>{let path=self.root.join("dictionary.json");if !path.exists(){return Ok(DictionaryData::default())}let bytes=fs::read(path).map_err(|e|format!("无法读取词库：{e}"))?;let data:DictionaryData=serde_json::from_slice(&bytes).map_err(|e|format!("词库文件无效：{e}"))?;if data.version>1{return Err(format!("词库版本 {} 高于当前支持的 1",data.version))}Ok(data)}
    pub fn save_dictionary(&self,hotwords:&[String],replacements:&[(String,String)])->Result<(),String>{let value=DictionaryData{version:1,hotwords:hotwords.to_vec(),replacements:replacements.to_vec()};let bytes=serde_json::to_vec_pretty(&value).map_err(|e|e.to_string())?;let tmp=self.root.join("dictionary.json.tmp");fs::write(&tmp,bytes).map_err(|e|e.to_string())?;fs::rename(tmp,self.root.join("dictionary.json")).map_err(|e|e.to_string())}
}

pub fn set_secret(account:&str,value:&str)->Result<(),String>{let password=security_framework::passwords::set_generic_password("pro.easyinput.desktop.intel",account,value.as_bytes());password.map_err(|e|e.to_string())}
pub fn get_secret(account:&str)->Result<Option<String>,String>{match security_framework::passwords::get_generic_password("pro.easyinput.desktop.intel",account){Ok(bytes)=>String::from_utf8(bytes).map(Some).map_err(|e|e.to_string()),Err(_)=>Ok(None)}}

#[cfg(test)] mod tests { use super::*; #[test] fn config_roundtrip(){let dir=std::env::temp_dir().join(format!("easyinput-test-{}",uuid::Uuid::new_v4()));let store=Storage::open(dir.clone()).unwrap();let cfg=store.read_config().unwrap();assert_eq!(cfg.version,4);store.write_config(&cfg).unwrap();let _=fs::remove_dir_all(dir);} #[test] fn migrates_legacy_doubao_2_resource(){let dir=std::env::temp_dir().join(format!("easyinput-test-{}",uuid::Uuid::new_v4()));let store=Storage::open(dir.clone()).unwrap();let mut cfg=PersistedConfig::default();cfg.version=1;cfg.speech.resource_id="volc.seedasr.sauc.duration".into();store.write_config(&cfg).unwrap();let migrated=store.read_config().unwrap();assert_eq!(migrated.version,4);assert_eq!(migrated.speech.resource_id,"volc.bigasr.sauc.duration");assert_eq!(migrated.ark.model,"doubao-seed-2-0-lite-260215");assert_eq!(migrated.realtime_voice.model,"1.2.6.1");let _=fs::remove_dir_all(dir);} #[test] fn history_roundtrip(){let dir=std::env::temp_dir().join(format!("easyinput-test-{}",uuid::Uuid::new_v4()));let store=Storage::open(dir.clone()).unwrap();let id=store.add_history("你好 EasyInput",1000,"Computer").unwrap();assert_eq!(store.history_page(None,10).unwrap()[0].id,id);store.delete_history(id).unwrap();let _=fs::remove_dir_all(dir);} }
