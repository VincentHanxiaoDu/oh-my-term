import { chromium, devices } from '@playwright/test'
import WebSocket from 'ws'
const url='http://127.0.0.1:7788'
const cred=await new Promise((res,rej)=>{const s=new WebSocket(url.replace(/^http/,'ws')+'/api/ws')
 s.on('open',()=>s.send(JSON.stringify({t:'join_exchange',request:{device:crypto.randomUUID(),n:1},invite:process.argv[2],device:{name:'r',platform:'t'}})))
 s.on('message',d=>{const m=JSON.parse(d.toString());s.close();m.token?res(m.token):rej(new Error(d.toString()))});s.on('error',rej)})
const b=await chromium.launch(); const ctx=await b.newContext({...devices['iPhone 13']})
const p=await ctx.newPage()
p.on('console', m => { if (m.type()==='error') console.log('CONSOLE ERROR:', m.text().slice(0,150)) })
p.on('pageerror', e => console.log('PAGE ERROR:', String(e).slice(0,150)))
await p.addInitScript(({t,u})=>{localStorage.setItem('omt.instances',JSON.stringify([{id:'local',url:u,token:t,label:'x'}]));localStorage.setItem('omt.instance','local')},{t:cred,u:url})
await p.goto(url, {waitUntil:'domcontentloaded'}); await p.waitForTimeout(2500)
console.log('loaded, now reloading')
try { await p.reload({ waitUntil: 'commit', timeout: 8000 }); console.log('reload committed') } catch (e) { console.log('RELOAD FAILED:', String(e).split('\n')[0]) }
await p.waitForTimeout(2000)
console.log('after:', (await p.innerText('body')).slice(0,100).replace(/\n/g,' | '))
await b.close()
