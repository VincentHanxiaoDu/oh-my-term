import { chromium, devices } from '@playwright/test'
import WebSocket from 'ws'
const url='http://127.0.0.1:7788'
const cred=await new Promise((res,rej)=>{const s=new WebSocket(url.replace(/^http/,'ws')+'/api/ws')
 s.on('open',()=>s.send(JSON.stringify({t:'join_exchange',request:{device:crypto.randomUUID(),n:1},invite:process.argv[2],device:{name:'r',platform:'t'}})))
 s.on('message',d=>{const m=JSON.parse(d.toString());s.close();m.token?res(m.token):rej(new Error(d.toString()))});s.on('error',rej)})
const b=await chromium.launch(); const ctx=await b.newContext({...devices['iPhone 13']})
const p=await ctx.newPage()
p.on('pageerror', e => console.log('PAGE ERROR:', String(e).split('\n')[0]))
p.on('console', m => { if (m.type()==='error') console.log('CONSOLE:', m.text().slice(0,180)) })
await p.addInitScript(({t,u})=>{localStorage.setItem('omt.instances',JSON.stringify([{id:'local',url:u,token:t,label:'x'}]));localStorage.setItem('omt.instance','local')},{t:cred,u:url})
await p.goto(url,{waitUntil:'commit'}); await p.waitForTimeout(2500)
await p.getByRole('button',{name:/Open a project/}).click(); await p.waitForTimeout(1000)
await p.getByRole('button',{name:/— Changes/}).first().click(); await p.waitForTimeout(1500)
console.log('BEFORE:', await p.locator('.bar h1').textContent(), '| state:', JSON.stringify(await p.evaluate(()=>history.state)).slice(0,120))
await p.reload({waitUntil:'commit'}); await p.waitForTimeout(3000)
console.log('AFTER state:', JSON.stringify(await p.evaluate(()=>history.state)).slice(0,120))
console.log('AFTER body:', (await p.innerText('body')).slice(0,150).replace(/\n/g,' | '))
await b.close()
