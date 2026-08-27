use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const MAX_HOTWORDS:usize=1000;
const MAX_FILE_BYTES:usize=1024*1024;
const MAX_WORD_CHARS:usize=100;

#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct DictionaryData{
 #[serde(default="dictionary_version")]
 pub version:u32,
 #[serde(default)]
 pub hotwords:Vec<String>,
 #[serde(default)]
 pub replacements:Vec<(String,String)>
}

fn dictionary_version()->u32{1}
impl Default for DictionaryData{fn default()->Self{Self{version:1,hotwords:Vec::new(),replacements:Vec::new()}}}

#[derive(Clone,Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub struct DictionaryImport{
 pub words:Vec<String>,
 pub blank_lines:u32,
 pub duplicate_lines:u32
}

#[derive(Clone,Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub struct DictionaryExport{
 pub path:String,
 pub count:usize
}

pub fn parse_text(bytes:&[u8])->Result<DictionaryImport,String>{
 if bytes.len()>MAX_FILE_BYTES{return Err("词库文件超过 1 MB，已拒绝导入".into())}
 let bytes=bytes.strip_prefix(&[0xef,0xbb,0xbf]).unwrap_or(bytes);
 let text=std::str::from_utf8(bytes).map_err(|_|"词库文件不是有效的 UTF-8 文本")?;
 let mut words=Vec::new();let mut seen=HashSet::new();let mut blank_lines=0;let mut duplicate_lines=0;
 for(raw_index,line)in text.lines().enumerate(){
  let word=line.trim();
  if word.is_empty(){blank_lines+=1;continue}
  if word.chars().count()>MAX_WORD_CHARS{return Err(format!("第 {} 行超过 {} 个字符",raw_index+1,MAX_WORD_CHARS))}
  if word.contains('\0'){return Err(format!("第 {} 行包含无效字符",raw_index+1))}
  if !seen.insert(word.to_owned()){duplicate_lines+=1;continue}
  words.push(word.to_owned());
  if words.len()>MAX_HOTWORDS{return Err(format!("词库超过 {MAX_HOTWORDS} 个热词"))}
 }
 if words.is_empty(){return Err("词库文件中没有可导入的热词".into())}
 Ok(DictionaryImport{words,blank_lines,duplicate_lines})
}

pub fn encode_text(hotwords:&[String])->Result<Vec<u8>,String>{
 if hotwords.len()>MAX_HOTWORDS{return Err(format!("热词数量超过 {MAX_HOTWORDS} 个"))}
 let mut normalized=Vec::new();let mut seen=HashSet::new();
 for(word_index,word)in hotwords.iter().enumerate(){
  let word=word.trim();if word.is_empty(){continue}
  if word.chars().any(|value|matches!(value,'\r'|'\n'|'\0')){return Err(format!("第 {} 个热词包含换行或无效字符",word_index+1))}
  if word.chars().count()>MAX_WORD_CHARS{return Err(format!("第 {} 个热词超过 {} 个字符",word_index+1,MAX_WORD_CHARS))}
  if seen.insert(word.to_owned()){normalized.push(word)}
 }
 if normalized.is_empty(){return Err("没有可导出的热词".into())}
 Ok(format!("{}\n",normalized.join("\n")).into_bytes())
}

pub fn apply_replacements(text:&str,replacements:&[(String,String)])->String{
 replacements.iter().fold(text.to_owned(),|current,(from,to)|{let from=from.trim();if from.is_empty(){current}else{current.replace(from,to)}})
}

#[cfg(test)]mod tests{
 use super::*;
 #[test]fn parses_attached_format_and_deduplicates(){let parsed=parse_text("\u{feff}信产\r\n信创云网\r\n\r\n信产\r\n".as_bytes()).unwrap();assert_eq!(parsed.words,vec!["信产","信创云网"]);assert_eq!(parsed.blank_lines,1);assert_eq!(parsed.duplicate_lines,1)}
 #[test]fn export_roundtrips(){let words=vec!["信产".into(),"信创云网".into()];let bytes=encode_text(&words).unwrap();assert_eq!(bytes,"信产\n信创云网\n".as_bytes());assert_eq!(parse_text(&bytes).unwrap().words,words)}
 #[test]fn rejects_non_utf8(){assert!(parse_text(&[0xff,0xfe]).is_err())}
 #[test]fn applies_rules_in_order_and_ignores_empty_source(){let rules=vec![("马威".into(),"马巍".into()),("".into(),"无效".into()),("信创".into(),"信创云网".into())];assert_eq!(apply_replacements("马威在做信创",&rules),"马巍在做信创云网")}
}
