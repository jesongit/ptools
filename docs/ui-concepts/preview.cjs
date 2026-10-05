const http = require('node:http');
const fs = require('node:fs');
const path = require('node:path');
const root = __dirname;
http.createServer((req,res)=>{
  let url;
  try { url = decodeURIComponent(new URL(req.url,'http://localhost').pathname); } catch { res.writeHead(400).end(); return; }
  const name=url==='/'?'index.html':url.slice(1);
  if(!/^[\w-]+\.html$/.test(name)){res.writeHead(404).end('Not found');return;}
  fs.readFile(path.join(root,name),(err,data)=>{
    if(err){res.writeHead(404).end('Not found');return;}
    res.writeHead(200,{'Content-Type':'text/html; charset=utf-8','Cache-Control':'no-store'});res.end(data);
  });
}).listen(4173,'127.0.0.1',()=>console.log('NOVA UI preview: http://127.0.0.1:4173'));
