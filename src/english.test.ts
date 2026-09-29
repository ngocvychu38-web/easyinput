import { describe, expect, it } from 'vitest';
import { ENGLISH_PROTOCOL, isLearningHello, isLearningRequest, resolveEnglishUrl } from './english';
const id='12345678-1234-1234-1234-123456789012';
const message=(action:unknown)=>({type:ENGLISH_PROTOCOL,clientId:id,requestId:id,token:id,ownerId:id,action});
describe('learning bridge trust boundary',()=>{
 it('accepts only bounded learning operations and UUID capabilities',()=>{
  expect(isLearningRequest(message({kind:'start',instructions:'Milo',greeting:''}))).toBe(true);
  expect(isLearningRequest(message({kind:'context',instructions:'中'.repeat(6000)}))).toBe(false);
  expect(isLearningRequest(message({kind:'text',text:''}))).toBe(false);
  expect(isLearningRequest(message({kind:'mute',muted:'false'}))).toBe(false);
  expect(isLearningRequest(message({kind:'invoke',command:'read_key'}))).toBe(false);
  expect(isLearningRequest({...message({kind:'stop'}),ownerId:'another-owner'})).toBe(false);
 });
 it('rejects malformed handshake and credential-bearing or insecure remote URLs',()=>{
  expect(isLearningHello({type:ENGLISH_PROTOCOL,kind:'hello',clientId:id})).toBe(true);
  expect(isLearningHello({type:ENGLISH_PROTOCOL,kind:'hello',clientId:'x'})).toBe(false);
  expect(resolveEnglishUrl()).toBe('http://localhost:3000/');
  expect(()=>resolveEnglishUrl('http://example.com')).toThrow();
  expect(()=>resolveEnglishUrl('https://user:secret@example.com')).toThrow();
  expect(()=>resolveEnglishUrl('javascript:alert(1)')).toThrow();
 });
});
