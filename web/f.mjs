import { chromium, devices } from '@playwright/test'
import WebSocket from 'ws'
const url='http://127.0.0.1:7788'
const cred=await new Promise((res,rej)=>{const s=new WebSocket(url.replace(/^http/,'ws')+'/api/ws')
 s.on('open',()=>s.send(JSON.stringify({t:'join_exchange',request:{device:crypto.randomUUID(),n:1},invite:process.argv[2],device:{name:'f',platform:'t'}})))
 s.on('message',d=>{const m=JSON.parse(d.toString());s.close();m.token?res(m.token):rej(new Error(d.toString()))});s.on('error',rej)})
const b=await chromium.launch(); const ctx=await b.newContext({...devices['iPhone 13'],colorScheme:'dark',serviceWorkers:'block'})
const p=await ctx.newPage()
await p.addInitScript(({t,u})=>{localStorage.setItem('omt.instances',JSON.stringify([{id:'local',url:u,token:t,label:'x'}]));localStorage.setItem('omt.instance','local')},{t:cred,u:url})
await p.goto(url); await p.waitForTimeout(2500)
await p.getByRole('button',{name:/Open a project/}).click(); await p.waitForTimeout(1200)
await p.getByRole('button',{name:/oh-my-term — Files/}).first().click(); await p.waitForTimeout(1800)
await p.locator('button', { hasText: 'README.md' }).first().click(); await p.waitForTimeout(2000)
await p.screenshot({path:'/tmp/s-file.png'})
console.log('FILE:', (await p.innerText('body')).slice(0,220).replace(/\n/g,' | '))
await b.close()
