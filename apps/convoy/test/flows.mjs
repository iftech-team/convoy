import { chromium } from "playwright";
import { createServer } from "vite";
import assert from "node:assert/strict";

// Exercise the real DOM with an in-memory IPC fixture. Backend lifecycle and
// real PTY behavior are covered by cargo test, not by these mocked calls.
const server = await createServer({server: {host: "127.0.0.1", port: 1421, strictPort: true}});
await server.listen();
let browser;
try {
 browser = await chromium.launch({executablePath: process.env.CONVOY_TEST_BROWSER || undefined, headless: true});
 const page = await browser.newPage({viewport:{width:1280,height:840}});
 const errors=[]; page.on('pageerror', e=>errors.push(e.message));
 await page.addInitScript(() => {
  const sessions = [{id:'builder',title:'Fix login',agent:'claude',provider_id:'id',started:true,running:true,review_of:null}];
  const projects=[]; const calls=[]; window.checkCalls=calls;
  window.__TAURI_INTERNALS__={transformCallback:()=>1,invoke:async (cmd,args)=>{
   calls.push({cmd,args});
   if(cmd==='settings_read')return {theme:'light',default_agent:'claude',font_size:13,scrollback:10000,claude_usage:false};
   if(cmd==='workspace_read')return {projects,running:['builder'],storage:'/fixture/workspace.json'};
   if(cmd==='sessions_for')return sessions;
   if(cmd==='tasks_for'||cmd==='integrations_list')return [];
   if(cmd==='project_add'){projects.push({id:'project',title:'Example',path:args.path,sessions:1,running:1});return 'project';}
   if(cmd==='review_create'){sessions.push({id:'review',title:'Review: Fix login',agent:'codex',provider_id:'',started:false,running:false,review_of:'builder'});return 'review';}
   if(cmd==='review_feedback')return 'builder';
   if(cmd==='plugin:event|listen')return 1;
   return null;
  }};
 });
 await page.goto('http://127.0.0.1:1421');
 await page.getByRole('button',{name:'Open folder…'}).click();
 await page.getByLabel('Full folder path').fill('/fixture/Example');
 await page.getByLabel('Full folder path').press('Enter');
 await page.getByRole('heading',{name:'Example',exact:true}).waitFor();
 await page.getByRole('button',{name:'Open the terminal'}).click();
 await page.getByRole('button',{name:'Review changes',exact:true}).click();
 await page.getByRole('button',{name:'Send findings to builder…'}).click();
 await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
 assert.equal(await page.evaluate(() => document.activeElement.id), "dialog-text", "the terminal must not take focus from the dialog");
 await page.getByLabel('Findings',{exact:true}).fill('Fix the error path before shipping.');
 await page.getByRole('button',{name:'Paste into builder'}).click();
 await page.getByRole('button',{name:'Review changes',exact:true}).waitFor();
 const calls=await page.evaluate(()=>window.checkCalls);
 if(!calls.some(c=>c.cmd==='review_feedback'&&c.args.id==='review'&&c.args.text==='Fix the error path before shipping.'))throw new Error('Missing feedback call');
 if(errors.length)throw new Error(errors.join('\n'));
 if (process.env.CONVOY_TEST_SCREENSHOT) await page.screenshot({path:process.env.CONVOY_TEST_SCREENSHOT});
 console.log('PASS: empty workspace → open folder with Enter → open builder → create review → paste findings back; no browser errors. Tauri IPC mocked.');
} finally {
 await browser?.close();
 await server.close();
}
