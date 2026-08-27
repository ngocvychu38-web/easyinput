use crate::model::{DoubaoSpeechConfig, OperationResult, RecordingPhase, RecordingState};
use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use std::{io::{Read, Write}, time::Instant};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, http::HeaderValue, Message};

pub const DOUBAO_ENDPOINT:&str="wss://openspeech.bytedance.com/api/v3/sauc/bigmodel";
pub const TOKEN_ACCOUNT:&str="doubao-asr-access-token";
const RESOURCE_IDS:[&str;4]=["volc.seedasr.sauc.duration","volc.seedasr.sauc.concurrent","volc.bigasr.sauc.duration","volc.bigasr.sauc.concurrent"];
const FULL_CLIENT_REQUEST:u8=0x1;
const AUDIO_ONLY_REQUEST:u8=0x2;
const FULL_SERVER_RESPONSE:u8=0x9;
const ERROR_RESPONSE:u8=0xf;

#[derive(Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub struct ConnectionTest { pub latency_ms:u128,pub endpoint:String,pub resource_id:String,pub log_id:Option<String> }

#[derive(Clone,Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub struct TranscriptEvent { pub session_id:String,pub text:String,pub definite:bool,pub sequence:Option<i32> }

#[derive(Clone,Debug,Serialize)]
#[serde(rename_all="camelCase")]
pub struct SessionEvent { pub session_id:String,pub phase:RecordingPhase,pub text:String,pub duration_ms:u64,pub message:Option<String> }

#[derive(Debug)]
pub enum StreamCommand { Audio(Vec<u8>), Finish }

pub fn validate(config:&DoubaoSpeechConfig)->Result<(),String>{
 if config.endpoint!=DOUBAO_ENDPOINT{return Err("为避免 Access Token 泄露，服务地址必须使用豆包语音官方 WSS 地址".into())}
 if config.app_key.trim().is_empty(){return Err("请填写 App Key".into())}
 if !RESOURCE_IDS.contains(&config.resource_id.as_str()){return Err("资源 ID 不在支持列表中".into())}
 if config.model_name!="bigmodel"{return Err("当前接口只支持 bigmodel".into())}
 if config.language!="zh-CN"{return Err("当前版本只实现中文普通话".into())}
 Ok(())
}

fn format_connect_error(error:tokio_tungstenite::tungstenite::Error)->String{
 if let tokio_tungstenite::tungstenite::Error::Http(response)=&error{
  let status=response.status();
  let body=response.body().as_deref().and_then(|v|std::str::from_utf8(v).ok()).map(str::trim).filter(|v|!v.is_empty());
  let log_id=response.headers().get("X-Tt-Logid").and_then(|v|v.to_str().ok());
  let hint=if status.as_u16()==400&&body.is_some_and(|v|v.contains("resourceId")){"；当前 Resource ID 未被账号允许，请在控制台确认开通的是 2.0/1.0、小时版还是并发版"}else if matches!(status.as_u16(),401|403){"；请检查 App Key、Access Token 与 Resource ID 是否属于同一个豆包语音应用"}else{""};
  return format!("豆包语音握手失败（HTTP {}）：{}{}{}",status.as_u16(),body.unwrap_or("服务端未返回错误正文"),hint,log_id.map(|v|format!("；LogID：{v}")).unwrap_or_default())
 }
 format!("豆包语音握手失败：{error}")
}

fn authorized_request(config:&DoubaoSpeechConfig,token:&str)->Result<tokio_tungstenite::tungstenite::http::Request<()>,String>{
 let mut request=config.endpoint.clone().into_client_request().map_err(|e|format!("服务地址无效：{e}"))?;
 let headers=request.headers_mut();
 for(name,value)in[("X-Api-App-Key",config.app_key.as_str()),("X-Api-Access-Key",token),("X-Api-Resource-Id",config.resource_id.as_str())]{
  let value=HeaderValue::from_str(value).map_err(|_|format!("{name} 包含无效字符"))?;
  headers.insert(name,value);
 }
 let connect_id=uuid::Uuid::new_v4().to_string();
 headers.insert("X-Api-Connect-Id",HeaderValue::from_str(&connect_id).map_err(|e|e.to_string())?);
 Ok(request)
}

pub async fn test_connection(config:&DoubaoSpeechConfig,token:&str)->OperationResult<ConnectionTest>{
 if let Err(e)=validate(config){return OperationResult::failure(e)}
 if token.trim().is_empty(){return OperationResult::failure("Access Token 为空")}
 let _=rustls::crypto::ring::default_provider().install_default();
 let request=match authorized_request(config,token){Ok(v)=>v,Err(e)=>return OperationResult::failure(e)};
 let started=Instant::now();let outcome=tokio::time::timeout(std::time::Duration::from_secs(10),tokio_tungstenite::connect_async(request)).await;
 match outcome{
  Ok(Ok((socket,response)))=>{let log_id=response.headers().get("X-Tt-Logid").and_then(|v|v.to_str().ok()).map(str::to_owned);drop(socket);OperationResult::success(Some(ConnectionTest{latency_ms:started.elapsed().as_millis(),endpoint:config.endpoint.clone(),resource_id:config.resource_id.clone(),log_id}))},
  Ok(Err(e))=>OperationResult::failure(format_connect_error(e)),
  Err(_)=>OperationResult::failure("连接豆包语音服务超时（10 秒）")
 }
}

fn gzip(payload:&[u8])->Result<Vec<u8>,String>{
 let mut encoder=GzEncoder::new(Vec::new(),Compression::default());
 encoder.write_all(payload).map_err(|e|format!("压缩语音数据失败：{e}"))?;
 encoder.finish().map_err(|e|format!("压缩语音数据失败：{e}"))
}

fn request_packet(message_type:u8,flags:u8,serialization:u8,sequence:Option<i32>,payload:&[u8])->Result<Vec<u8>,String>{
 let compressed=gzip(payload)?;
 let mut packet=Vec::with_capacity(12+compressed.len());
 packet.extend_from_slice(&[0x11,(message_type<<4)|flags,(serialization<<4)|0x1,0x00]);
 if let Some(sequence)=sequence{packet.extend_from_slice(&sequence.to_be_bytes());}
 packet.extend_from_slice(&(compressed.len() as u32).to_be_bytes());
 packet.extend_from_slice(&compressed);
 Ok(packet)
}

fn initial_packet(config:&DoubaoSpeechConfig)->Result<Vec<u8>,String>{
 let payload=serde_json::json!({
  "user":{"uid":config.app_key},
  "audio":{"format":"pcm","rate":16000,"bits":16,"channel":1,"codec":"raw"},
  "request":{"model_name":config.model_name,"enable_itn":config.enable_itn,"enable_punc":config.enable_punc,"show_utterances":config.show_utterances}
 });
 request_packet(FULL_CLIENT_REQUEST,1,1,Some(1),&serde_json::to_vec(&payload).map_err(|e|e.to_string())?)
}

fn audio_packet(pcm:&[u8],last:bool)->Result<Vec<u8>,String>{request_packet(AUDIO_ONLY_REQUEST,if last{2}else{0},0,None,pcm)}

#[derive(Debug)]
struct ServerPacket { sequence:Option<i32>,json:serde_json::Value,is_final:bool }

fn parse_server_packet(raw:&[u8])->Result<Option<ServerPacket>,String>{
 if raw.len()<8{return Err("豆包语音返回了不完整的数据包".into())}
 let header_len=((raw[0]&0x0f)as usize)*4;
 if header_len<4||raw.len()<header_len{return Err("豆包语音返回头长度无效".into())}
 let message_type=raw[1]>>4;let flags=raw[1]&0x0f;let compression=raw[2]&0x0f;let mut offset=header_len;
 let sequence=if flags&1!=0{if raw.len()<offset+4{return Err("豆包语音返回序号缺失".into())}let value=i32::from_be_bytes(raw[offset..offset+4].try_into().unwrap());offset+=4;Some(value)}else{None};
 if message_type==ERROR_RESPONSE{
  if raw.len()<offset+8{return Err("豆包语音返回了不完整的错误包".into())}
  let code=u32::from_be_bytes(raw[offset..offset+4].try_into().unwrap());offset+=4;
  let size=u32::from_be_bytes(raw[offset..offset+4].try_into().unwrap())as usize;offset+=4;
  let message=raw.get(offset..offset+size).and_then(|v|std::str::from_utf8(v).ok()).unwrap_or("未知错误");
  return Err(format!("豆包语音识别失败（{code}）：{message}"))
 }
 if message_type!=FULL_SERVER_RESPONSE{return Ok(None)}
 if raw.len()<offset+4{return Err("豆包语音返回正文长度缺失".into())}
 let size=u32::from_be_bytes(raw[offset..offset+4].try_into().unwrap())as usize;offset+=4;
 let payload=raw.get(offset..offset+size).ok_or("豆包语音返回正文不完整")?;
 let decoded=if compression==1{let mut decoder=GzDecoder::new(payload);let mut out=Vec::new();decoder.read_to_end(&mut out).map_err(|e|format!("解压豆包语音响应失败：{e}"))?;out}else{payload.to_vec()};
 let json=serde_json::from_slice(&decoded).map_err(|e|format!("豆包语音响应 JSON 无效：{e}"))?;
 Ok(Some(ServerPacket{sequence,json,is_final:sequence.is_some_and(|v|v<0)||flags&2!=0}))
}

fn transcript_from_json(value:&serde_json::Value)->Option<(String,bool)>{
 let candidates=[value.pointer("/result/0"),value.pointer("/result"),value.pointer("/payload/result/0"),value.pointer("/payload")];
 for candidate in candidates.into_iter().flatten(){
  if let Some(text)=candidate.get("text").and_then(|v|v.as_str()).filter(|v|!v.is_empty()){
   let definite=candidate.get("utterances").and_then(|v|v.as_array()).is_some_and(|items|!items.is_empty()&&items.iter().all(|item|item.get("definite").and_then(|v|v.as_bool()).unwrap_or(false)));
   return Some((text.to_owned(),definite))
  }
 }
 None
}

fn update_transcript(app:&AppHandle,session_id:&str,text:&str,elapsed_ms:u64){
 let state=app.state::<crate::AppState>();
 if let Ok(mut recording)=state.recording.lock(){if recording.session_id.as_deref()==Some(session_id){recording.partial_text=text.to_owned();recording.elapsed_ms=elapsed_ms}};
}

async fn finish_session(app:&AppHandle,session_id:&str,mut text:String,duration_ms:u64,mut error:Option<String>,source:&str){
 let state=app.state::<crate::AppState>();
 if let Ok(mut sessions)=state.speech_sessions.lock(){sessions.remove(session_id);}
 if error.is_none()&&source=="KeyboardEdit"{
  if text.trim().is_empty(){error=Some("语音编辑没有识别到有效问题".into())}else{
   let selected=state.edit_context.lock().ok().and_then(|mut value|value.take());
   match state.storage.read_config(){
    Ok(config)=>match crate::read_ark_api_key(state.inner()).await{
     Ok(api_key)=>match crate::ark::answer(&config.ark,&api_key,text.trim(),selected.as_deref()).await{
      Ok(answer)=>{text=answer;if let Err(input_error)=crate::input::type_text(text.trim()){error=Some(input_error)}},
      Err(model_error)=>error=Some(model_error),
     },
     Err(key_error)=>error=Some(key_error),
    },
    Err(config_error)=>error=Some(config_error),
   }
  }
 }
 if error.is_none()&&!text.trim().is_empty(){let _=state.storage.add_history(text.trim(),duration_ms,source);if source=="Keyboard"{if let Err(input_error)=crate::input::type_text(text.trim()){error=Some(input_error)}}}
 let message=error.clone();
 let phase=if error.is_some(){RecordingPhase::Error}else{RecordingPhase::Idle};
 if let Ok(mut recording)=state.recording.lock(){if recording.session_id.as_deref()==Some(session_id){*recording=RecordingState{phase:phase.clone(),session_id:None,elapsed_ms:duration_ms,partial_text:text.clone(),error:message.clone()}}}
 let _=app.emit("speech-session",SessionEvent{session_id:session_id.to_owned(),phase,text,duration_ms,message});
}

async fn handle_message(app:&AppHandle,session_id:&str,message:Message,started:Instant,latest:&mut String,replacements:&[(String,String)])->Result<bool,String>{
 match message{
  Message::Binary(raw)=>if let Some(packet)=parse_server_packet(&raw)?{
   if let Some((text,definite))=transcript_from_json(&packet.json){let text=crate::dictionary::apply_replacements(&text,replacements);
    *latest=text.clone();let elapsed=started.elapsed().as_millis()as u64;update_transcript(app,session_id,&text,elapsed);
    let _=app.emit("speech-transcript",TranscriptEvent{session_id:session_id.to_owned(),text,definite,sequence:packet.sequence});
   }
   Ok(packet.is_final)
  }else{Ok(false)},
  Message::Close(frame)=>Err(frame.map(|v|format!("豆包语音连接已关闭：{}",v.reason)).unwrap_or_else(||"豆包语音连接已关闭".into())),
  _=>Ok(false)
 }
}

pub async fn open_stream(app:AppHandle,session_id:String,config:DoubaoSpeechConfig,token:String,source:String)->Result<mpsc::UnboundedSender<StreamCommand>,String>{
 validate(&config)?;if token.trim().is_empty(){return Err("Access Token 为空".into())}
 let _=rustls::crypto::ring::default_provider().install_default();
 let request=authorized_request(&config,&token)?;
 let (mut socket,_)=tokio::time::timeout(std::time::Duration::from_secs(10),tokio_tungstenite::connect_async(request)).await.map_err(|_|"连接豆包语音服务超时（10 秒）".to_string())?.map_err(format_connect_error)?;
 socket.send(Message::Binary(initial_packet(&config)?.into())).await.map_err(|e|format!("发送豆包语音初始化请求失败：{e}"))?;
 let replacements=app.state::<crate::AppState>().storage.read_dictionary().map(|data|data.replacements).unwrap_or_default();
 let(tx,mut rx)=mpsc::unbounded_channel();
 tauri::async_runtime::spawn(async move{
  let started=Instant::now();let mut latest=String::new();let mut error=None;
  loop{
   tokio::select!{
    command=rx.recv()=>match command{
     Some(StreamCommand::Audio(pcm))=>if let Err(e)=socket.send(Message::Binary(match audio_packet(&pcm,false){Ok(v)=>v.into(),Err(e)=>{error=Some(e);break}})).await{error=Some(format!("发送语音数据失败：{e}"));break},
     Some(StreamCommand::Finish)|None=>{if let Err(e)=socket.send(Message::Binary(match audio_packet(&[],true){Ok(v)=>v.into(),Err(e)=>{error=Some(e);break}})).await{error=Some(format!("结束语音会话失败：{e}"))}break}
    },
    incoming=socket.next()=>match incoming{
     Some(Ok(message))=>match handle_message(&app,&session_id,message,started,&mut latest,&replacements).await{Ok(true)=>break,Ok(false)=>{},Err(e)=>{error=Some(e);break}},
     Some(Err(e))=>{error=Some(format!("接收豆包语音结果失败：{e}"));break},
     None=>{error=Some("豆包语音连接意外中断".into());break}
    }
   }
  }
  if error.is_none(){
   let deadline=tokio::time::Instant::now()+std::time::Duration::from_secs(5);
   loop{match tokio::time::timeout_at(deadline,socket.next()).await{
    Ok(Some(Ok(message)))=>match handle_message(&app,&session_id,message,started,&mut latest,&replacements).await{Ok(true)=>break,Ok(false)=>continue,Err(e)=>{error=Some(e);break}},
    Ok(Some(Err(e)))=>{error=Some(format!("接收最终转写失败：{e}"));break},
    Ok(None)=>break,
    Err(_)=>{if latest.is_empty(){error=Some("等待豆包语音最终结果超时（5 秒）".into())}break}
   }}
  }
  let duration=started.elapsed().as_millis()as u64;finish_session(&app,&session_id,latest,duration,error,&source).await;
 });
 Ok(tx)
}

#[cfg(test)]mod tests{
 use super::*;
 #[test]fn defaults_are_official_and_safe(){let c=DoubaoSpeechConfig{app_key:"demo".into(),..Default::default()};assert_eq!(c.endpoint,DOUBAO_ENDPOINT);assert_eq!(c.resource_id,"volc.bigasr.sauc.duration");assert!(validate(&c).is_ok())}
 #[test]fn rejects_credential_exfiltration_endpoint(){let c=DoubaoSpeechConfig{app_key:"demo".into(),endpoint:"wss://example.com/steal".into(),..Default::default()};assert!(validate(&c).is_err())}
 #[test]fn protocol_initial_packet_is_v1_json_gzip_with_sequence(){let c=DoubaoSpeechConfig{app_key:"demo".into(),..Default::default()};let packet=initial_packet(&c).unwrap();assert_eq!(&packet[..4],&[0x11,0x11,0x11,0]);assert_eq!(i32::from_be_bytes(packet[4..8].try_into().unwrap()),1);let size=u32::from_be_bytes(packet[8..12].try_into().unwrap())as usize;assert_eq!(size,packet.len()-12);}
 #[test]fn parses_gzip_server_transcript(){let json=serde_json::json!({"result":[{"text":"你好 EasyInput","utterances":[{"definite":true}]}]});let payload=gzip(&serde_json::to_vec(&json).unwrap()).unwrap();let mut raw=vec![0x11,0x93,0x11,0];raw.extend_from_slice(&(-7i32).to_be_bytes());raw.extend_from_slice(&(payload.len()as u32).to_be_bytes());raw.extend_from_slice(&payload);let parsed=parse_server_packet(&raw).unwrap().unwrap();assert!(parsed.is_final);assert_eq!(transcript_from_json(&parsed.json),Some(("你好 EasyInput".into(),true)));}
 #[tokio::test]
 #[ignore = "访问豆包官方端点的手动网络诊断"]
 async fn official_endpoint_probe_returns_within_timeout(){let c=DoubaoSpeechConfig{app_key:"invalid-probe".into(),..Default::default()};let started=std::time::Instant::now();let result=test_connection(&c,"invalid-probe").await;println!("probe elapsed={:?} ok={} message={:?}",started.elapsed(),result.ok,result.message);assert!(started.elapsed()<std::time::Duration::from_secs(15));assert!(!result.ok)}
}
