/* Offline, deterministic 1080p motion graphics. No remote assets or browser. */
const fs = require('fs');
const path = require('path');
const {spawn} = require('child_process');
const {once} = require('events');
const deps = process.env.CODEX_NODE_MODULES || '/Users/mineralsteins/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules';
const {createCanvas, GlobalFonts} = require(path.join(deps, '@napi-rs/canvas'));
const out = path.resolve(process.argv[2]);
const preview = process.argv.includes('--preview');
const film = JSON.parse(fs.readFileSync(path.join(out,'timeline.json'),'utf8'));
const ir = JSON.parse(fs.readFileSync(path.join(out,'ir-demo.json'),'utf8'));
const W=1920,H=1080,FPS=film.fps;
const canvas=createCanvas(W,H),g=canvas.getContext('2d');
GlobalFonts.registerFromPath('/System/Library/Fonts/SFNS.ttf','SF Pro');
const C={bg:'#08121f',panel:'#101f31',line:'#26394e',text:'#eff5fc',muted:'#93a9bf',cyan:'#55e1e6',mint:'#b7ef99',violet:'#afa4ff',amber:'#ffc181',red:'#ff8297'};
const font='"SF Pro", "Arial", "Heiti TC", sans-serif';
const mono='"Menlo", "Arial Unicode MS", monospace';
const clamp=(x,a=0,b=1)=>Math.min(b,Math.max(a,x));
const ease=x=>1-Math.pow(1-clamp(x),3);
function text(s,x,y,size=30,color=C.text,weight=400,align='left',family=font){weight=Math.round(weight/100)*100;g.font=`${weight} ${size}px ${family}`;g.fillStyle=color;g.textAlign=align;g.textBaseline='middle';g.fillText(s,x,y);}
function round(x,y,w,h,r=20,fill=C.panel,stroke=null,lw=1.5){g.beginPath();g.roundRect(x,y,w,h,r);if(fill){g.fillStyle=fill;g.fill();}if(stroke){g.strokeStyle=stroke;g.lineWidth=lw;g.stroke();}}
function pill(s,x,y,w,color=C.cyan){round(x,y,w,42,21,color+'16',color+'55');text(s,x+w/2,y+21,20,color,600,'center');}
function line(pts,color=C.line,width=3,p=1,dashed=false){
  let lengths=pts.slice(1).map((q,i)=>Math.hypot(q[0]-pts[i][0],q[1]-pts[i][1]));let left=lengths.reduce((a,b)=>a+b,0)*clamp(p);
  g.save();g.strokeStyle=color;g.lineWidth=width;g.lineJoin='round';g.lineCap='round';if(dashed)g.setLineDash([10,9]);g.beginPath();g.moveTo(...pts[0]);
  for(let i=0;i<lengths.length;i++){let a=pts[i],b=pts[i+1],t=Math.min(1,left/lengths[i]);g.lineTo(a[0]+(b[0]-a[0])*t,a[1]+(b[1]-a[1])*t);left-=lengths[i];if(left<=0)break;}g.stroke();g.restore();
}
function arrow(a,b,color=C.cyan,p=1){line([a,b],color,3,p);if(p>=.99){let t=Math.atan2(b[1]-a[1],b[0]-a[0]);g.save();g.translate(...b);g.rotate(t);g.fillStyle=color;g.beginPath();g.moveTo(0,0);g.lineTo(-13,-6);g.lineTo(-13,6);g.closePath();g.fill();g.restore();}}
function glowDot(x,y,color=C.cyan,r=7){g.save();g.shadowColor=color;g.shadowBlur=18;g.fillStyle=color;g.beginPath();g.arc(x,y,r,0,Math.PI*2);g.fill();g.restore();}
function flow(a,b,t,color=C.cyan){const u=((t*.22)%1+1)%1;glowDot(a[0]+(b[0]-a[0])*u,a[1]+(b[1]-a[1])*u,color,5);}
function node(x,y,w,h,title,sub='',color=C.cyan,active=false){
  g.save();if(active){g.shadowColor=color+'77';g.shadowBlur=25;}round(x,y,w,h,19,active?color+'18':C.panel,active?color:C.line,active?2.5:1.5);g.restore();
  text(title,x+w/2,y+h/2-(sub?13:0),30,C.text,600,'center');if(sub)text(sub,x+w/2,y+h/2+23,20,active?color:C.muted,400,'center');
}
function circle(x,y,r,label,color=C.cyan,active=false){g.beginPath();g.arc(x,y,r,0,Math.PI*2);g.fillStyle=active?color+'20':C.panel;g.fill();g.strokeStyle=active?color:C.line;g.lineWidth=2;g.stroke();text(label,x,y,28,active?color:C.text,600,'center');}
function reveal(a,fn){g.save();g.globalAlpha*=clamp(a);fn();g.restore();}
function smallNote(s,color=C.muted){text(s,100,854,22,color);}
function stageOf(ch,frame){let ids=ch.clips,stage=0,progress=0;for(let i=0;i<ids.length;i++){let c=film.clips[ids[i]];if(frame>=c.start){stage=i;progress=clamp((frame-c.start)/c.frames);}}return{stage,progress};}
function panelTitle(s,x,y,color=C.cyan){text(s,x,y,21,color,700);}
function table(x,y,w,rows,{title='',active=-1,highlight=C.cyan}={}){
  round(x,y,w,64+rows.length*61,18,C.panel,C.line);if(title)text(title,x+24,y+31,23,C.muted,600);
  rows.forEach((r,i)=>{let yy=y+64+i*61;if(i===active)round(x+8,yy+3,w-16,55,9,highlight+'18');line([[x+20,yy],[x+w-20,yy]],C.line,1);text(r[0],x+27,yy+30,25,C.text,500);text(r[1],x+w-28,yy+30,28,i===active?highlight:C.text,600,'right');});
}
function background(frame){
  const grad=g.createLinearGradient(0,0,W,H);grad.addColorStop(0,'#07111d');grad.addColorStop(.5,'#0c1a2b');grad.addColorStop(1,'#08121e');g.fillStyle=grad;g.fillRect(0,0,W,H);
  g.strokeStyle='#28405620';g.lineWidth=1;g.beginPath();for(let x=0;x<W;x+=64){g.moveTo(x,0);g.lineTo(x,H);}for(let y=0;y<H;y+=64){g.moveTo(0,y);g.lineTo(W,y);}g.stroke();
  const halo=g.createRadialGradient(1550,360,20,1550,360,750);halo.addColorStop(0,'#3e92ad10');halo.addColorStop(1,'#3e92ad00');g.fillStyle=halo;g.fillRect(0,0,W,H);
}
function header(ch,index,frame){
  round(88,54,8,22,4,C.cyan);text('FRACTAL EGRAPH  /  RESEARCH FILM',112,66,19,C.muted,650);
  text(ch.section,1818,66,22,C.cyan,600,'right');
  let titleSize=57;g.font=`700 ${titleSize}px ${font}`;while(g.measureText(ch.title).width>1700 && titleSize>38){titleSize--;g.font=`700 ${titleSize}px ${font}`;}
  text(ch.title,98,153,titleSize,C.text,650);text(ch.subtitle,101,215,21,C.muted,500);
  line([[100,256],[1820,256]],C.line,1);
  text(`${String(index+1).padStart(2,'0')} / ${film.chapters.length}`,1818,854,19,C.muted,500,'right',mono);
}
function scene(ch,frame){
  const t=(frame-ch.start)/FPS,{stage:s,progress:p}=stageOf(ch,frame),u=ease(t/1.4);
  if(ch.id==='intro'){
    text('STATEWALK',100,383,116,C.cyan,700);text('合法路径 × 共享成本',109,513,52,C.text,600);
    text('让图中的选择，成为可执行的程序',111,596,29,C.muted);
    pill(`中文图解 · 约${Math.round(film.duration/60)}分钟`,110,697,240,C.cyan);pill('本机合成配音',370,697,200,C.violet);
    const xs=[1200,1510,1740],ys=[420,330,510];
    arrow([xs[0]+65,ys[0]-5],[xs[1]-68,ys[1]+15],C.cyan,u);arrow([xs[0]+62,ys[0]+24],[xs[2]-69,ys[2]-20],C.violet,u);
    arrow([xs[1]+30,ys[1]+61],[xs[2]-30,ys[2]-61],C.mint,u);
    circle(xs[0],ys[0],67,'状态',C.cyan,true);circle(xs[1],ys[1],67,'选择',C.mint,true);circle(xs[2],ys[2],67,'共享',C.violet,true);
    flow([1265,415],[1442,345],t);flow([1262,444],[1671,490],t+.7,C.violet);
    text('算法不变 · 数据结构更轻',1500,690,27,C.muted,400,'center');
  } else if(ch.id==='problem'){
    panelTitle('纯表达式：有多种等价实现',120,307);round(112,350,750,392,26,C.panel,C.line);
    pill('同一个 e-class',340,380,290,C.mint);
    node(168,471,292,93,'x·(a+b)','一种实现',C.mint,s===0);node(515,471,292,93,'x·a + x·b','另一种实现',C.mint,s===0);
    text('=',488,517,40,C.mint,600,'center');text('从候选中选择实现',487,664,30,C.muted,400,'center');
    panelTitle('副作用：必须保持合法依赖',1010,307,C.violet);
    node(1000,396,235,100,'读取','Read',C.violet,s>=1);node(1291,396,235,100,'写入','Write',C.violet,s>=1);node(1582,396,235,100,'打印','Print',C.violet,s>=1);
    arrow([1235,446],[1290,446],C.violet);arrow([1526,446],[1581,446],C.violet);
    round(1010,575,792,127,20,'#291d2b',C.red+'60');text('便宜 ≠ 可以随意拼接',1406,619,37,C.red,600,'center');text('需要的值必须先能得到',1406,666,27,C.muted,400,'center');
    smallNote('Extraction：在等价候选中，选出依赖与副作用都合法的程序。');
  } else if(ch.id==='pipeline'){
    const names=['全图成本估计','区域化','Statewalk','纯表达式提取'];const desc=['先估计候选开销','按控制流区域处理','选择合法副作用路径','保留共享，完成程序'];
    names.forEach((name,i)=>{let x=108+i*448;node(x,413,350,151,name,desc[i],i===2?C.violet:C.cyan,(t*0.45)%4>=i);if(i<3)arrow([x+354,488],[x+438,488],C.muted);});
    round(380,651,1160,93,20,C.amber+'10',C.amber+'55');text('包含贪心估计；不承诺整个 DAG 的全局最优',960,697,34,C.amber,600,'center');
    smallNote('后面的优化保留这条流程、原有成本规则与同分选择。');
  } else if(ch.id==='real_source'){
    panelTitle('真实 Rust 源码 · raytrace.rs:193',110,310,C.amber);
    round(110,352,975,345,22,C.panel,C.line);pill('vec_add',140,377,170,C.amber);
    ir.rust.split('\n').forEach((row,j)=>{text(String(ir.first_line+j),138,461+j*51,18,C.muted,400,'left',mono);text(row,194,461+j*51,23,j===1?C.cyan:C.text,500,'left',mono);});
    text('数组地址 + 状态，进入底层 IR',140,646,28,C.muted);
    node(1210,337,530,93,'Bop(Load, ptr, state)','真实读取算子',C.violet,s>=1);
    arrow([1475,434],[1475,484],C.violet);node(1220,490,510,83,'(value, next_state)','读取结果是二元组',C.cyan,s>=1);
    arrow([1330,577],[1260,646],C.cyan);arrow([1620,577],[1680,646],C.violet);
    node(1120,650,280,103,'Get(load, 0)','数值',C.cyan,s>=1);node(1535,650,280,103,'Get(load, 1)','后继状态',C.violet,s>=1);
    smallNote('源码与节点来自真实保存快照。状态线表达执行依赖，并不复制整个内存。');
  } else if(ch.id==='real_ir'){
    panelTitle('Tiger 原始输出 · 保留原节点 ID',110,309,C.amber);
    round(108,348,1085,439,22,C.panel,C.line);
    ir.lines.forEach((row,j)=>{let focus=s===0?(j===2||j===3||j===4):s===1?(j===3||j===6):j===4;
      if(focus)round(118,372+j*55,1064,47,8,(j===6?C.cyan:C.violet)+'16');
      text(String(row.line),131,396+j*55,17,C.muted,400,'left',mono);text(row.text,199,396+j*55,22,focus?(j===6?C.cyan:C.violet):C.text,400,'left',mono);
    });
    node(1270,348,500,95,'__tmp7068','第一次 Load 的结果',C.violet,true);
    arrow([1390,448],[1374,526],C.cyan);arrow([1660,448],[1660,642],C.violet);
    node(1245,533,343,87,'Get(_, 0)','数值 → 后面的 FAdd',C.cyan,s>=1);
    node(1455,648,343,87,'Get(_, 1)','状态 → 下一次 Load',C.violet,true);
    smallNote('省略了不相关定义；可下载完整 vec_add 提取 IR。这是一条已选中的路径。');
  } else if(ch.id==='real_path'){
    let ready=s===0?Math.min(5,Math.floor(p*6)):5;
    panelTitle('紫色：真实的状态依赖顺序（相邻 Load 通过 Get(_,1) 连接）',110,301,C.violet);
    ir.loads.forEach((r,j)=>{let x=110+j*285;if(j<5)arrow([x+240,393],[x+281,393],j<ready?C.violet:C.line);
      node(x,345,240,96,`Load ${r.label}`,r.node,C.violet,j<=ready);
    });
    const outputs=[{x:165,label:'a[0] + b[0]',after:5,color:C.cyan,n:6,roots:[0,5]},
      {x:740,label:'a[1] + b[1]',after:2,color:C.mint,n:3,roots:[1,2]},
      {x:1315,label:'a[2] + b[2]',after:4,color:C.amber,n:5,roots:[3,4]}];
    outputs.forEach(o=>{
      for(let k=0;k<2;k++){let j=o.roots[k],start=[230+j*285,446],end=[o.x+100+k*210,552];line([start,[start[0],481+k*22],[end[0],481+k*22],end],j<=ready?o.color+'99':C.line,2);}
      node(o.x,557,410,89,o.label,`第 ${o.n} 次读取后可提取`,o.color,ready>=o.after);
    });
    line([[1780,393],[1844,393],[1844,704],[340,704],[340,732]],s>=2?C.violet:C.line,2.5);
    if(s>=2){
      arrow([340,711],[340,732],C.violet);
      outputs.forEach((o,j)=>arrow([o.x+205,650],[775+j*435,732],o.color));
    }
    const labels=['Alloc 结果数组','Write 分量0','Write 分量1','Write 分量2'];
    labels.forEach((v,j)=>{let x=165+j*435;node(x,738,350,73,v,'',C.violet,s>=2);if(j<3)arrow([x+355,775],[x+428,775],s>=2?C.violet:C.line);});
    smallNote(s>=2?'真实共享点：__tmp7097 = Get(Alloc,0)，供三次 PtrAdd 和返回值共同使用。':'第一分量的加法反而最后准备好：状态路径与纯计算就绪顺序，并不相同。',s>=2?C.cyan:C.muted);
  } else if(ch.id==='state'){
    pill('同一个当前位置 m₂',109,309,365,C.violet);
    node(120,392,660,144,'状态 A','位置 m₂  ·  可提取 {x, p}  ·  累计估价 5',C.cyan,true);
    node(120,580,660,144,'状态 B','位置 m₂  ·  可提取 {x, q}  ·  累计估价 4',C.violet,true);
    node(1110,469,605,132,'后续操作需要 p','A 满足依赖；B 暂不满足',C.mint,s>=1);
    arrow([780,460],[1095,510],s>=1?C.mint:C.line);line([[780,652],[1035,652],[1100,566]],s>=1?C.red:C.line,3,1,true);
    if(s>=1){pill('可继续',851,404,145,C.mint);pill('不能只因成本低就替代 A',818,704,420,C.red);}
    smallNote('“可提取”是可用性信息，不是最终已经选择的纯表达式集合。',s>=2?C.amber:C.muted);
  } else if(ch.id==='walk'){
    let readReady=s>=1||p>.34,pureReady=s>=1,writeReady=s>=2;
    panelTitle('副作用路径',111,315,C.violet);panelTitle('纯计算的可提取性',111,574,C.mint);
    circle(182,442,52,'m₀',C.violet,true);node(355,392,270,100,'读取 A','Load',C.violet,readReady);circle(780,442,52,'m₁',C.violet,readReady);
    node(1040,392,290,100,'写入 y','Store',C.violet,writeReady);circle(1510,442,52,'m₂',C.violet,writeReady);
    arrow([234,442],[351,442],C.violet);arrow([629,442],[724,442],readReady?C.violet:C.line);arrow([836,442],[1034,442],writeReady?C.violet:C.line);arrow([1334,442],[1452,442],writeReady?C.violet:C.line);
    line([[490,496],[490,652],[610,652]],readReady?C.cyan:C.line,3);node(610,603,235,98,'x','读取产生的值',C.cyan,readReady);
    arrow([849,652],[963,652],pureReady?C.mint:C.line);node(968,603,270,98,'y = x + 1','纯表达式',C.mint,pureReady);
    line([[1241,651],[1373,651],[1373,530],[1185,530],[1185,496]],writeReady?C.mint:C.line,3,1,!writeReady);
    pill(writeReady?'依赖就绪':'等待 y 可提取',1436,625,320,writeReady?C.mint:C.amber);
    if(readReady)flow([630,442],[724,442],t,C.violet);if(pureReady)flow([850,652],[962,652],t,C.mint);
    smallNote(s>=3?'纯闭包 = 传播已有节点的可提取性；不运行 rewrite，也不创建新 e-node。':'示意：先满足数据依赖，再推进副作用路径。',s>=3?C.amber:C.muted);
  } else if(ch.id==='dp'){
    panelTitle('候选状态',125,318);table(120,361,765,[['依赖不齐的候选','估价 2'],['合法候选 P','估价 6'],['合法候选 Q','估价 4']],{title:'先检查全部必要依赖',active:s===0?0:2,highlight:s===0?C.red:C.mint});
    if(s===0){line([[154,454],[840,454]],C.red,3);pill('不入队',623,662,220,C.red);}else pill('优先展开 Q',521,662,320,C.mint);
    panelTitle('动态规划记录',1030,318,C.violet);node(1023,380,755,100,'同位置 ＋ 同相关可提取信息','共享同一个后续搜索问题',C.violet,true);
    node(1051,540,290,100,'旧记录：7','',C.muted);node(1481,540,290,100,'新记录：4','保留',C.mint,s>=1);arrow([1355,590],[1467,590],C.mint);
    if(s>=2){pill('保存前驱 → 到达目标 → 回溯路径',1033,702,740,C.cyan);}
    smallNote('队列比较的是累计 Statewalk 估价；完整程序仍由后续提取阶段完成。');
  } else if(ch.id==='cache'){
    const cards=[['01','纯闭包缓存','相同状态 + 相同下一步','复用可提取性传播',C.cyan],['02','未来相关信息','忽略已不影响未来的值','减少需要区分的状态',C.mint],['03','持久化状态','多个搜索分支','共享未修改的部分',C.violet]];
    cards.forEach((c,i)=>{let x=110+i*579;round(x,365,540,368,25,C.panel,C.line);text(c[0],x+30,408,30,c[4],650);text(c[1],x+30,483,36,C.text,650);text(c[2],x+30,565,25,C.muted);text(c[3],x+30,626,26,c[4]);line([[x+30,679],[x+500,679]],c[4],3,ease((t-i*.4)/1.3));});
    smallNote('本次改动的目标：成本账表的复制与合并。不是重新发明这些 Statewalk 缓存。',s>=2?C.amber:C.muted);
  } else if(ch.id==='sharing'){
    node(575,325,330,105,'t：共享计算','成本 3 · 同一个 e-class',C.cyan,true);
    arrow([642,433],[405,569],C.cyan);arrow([844,433],[1069,569],C.cyan);
    node(270,574,300,110,'A 自身成本 1','加上 t，总成本 4',C.mint,true);node(955,574,300,110,'B 自身成本 2','加上 t，总成本 5',C.violet,true);
    flow([642,433],[405,569],t,C.cyan);flow([844,433],[1069,569],t,C.cyan);
    table(1395,340,390,[['t','3'],['A','1'],['B','2']],{title:'一起使用时的账表',active:0});
    if(s>=1){text('4 + 5 = 9',745,751,40,C.red,600,'center');line([[620,751],[872,751]],C.red,3);text('3 + 1 + 2 = 6',1590,664,36,C.mint,650,'center');}
    smallNote('教学成本；省略零成本输入。父节点自身成本另加。');
  } else if(ch.id==='representation'){
    panelTitle('之前：每次复制整张哈希账表',115,319,C.red);panelTitle('现在：复制指针，共享不可变账表',1000,319,C.mint);
    for(let i=0;i<3;i++){
      let x=115+i*270;node(x,375,230,70,`候选 ${i+1}`,'',C.red,false);arrow([x+115,450],[x+115,502],C.red);
      round(x,510,230,199,16,C.panel,C.red+'55');['t → 3','A → 1','…'].forEach((v,j)=>{round(x+18,530+j*51,194,39,7,'#342232');text(v,x+115,549+j*51,24,C.text,500,'center',mono);});
    }
    for(let i=0;i<3;i++){let x=1000+i*280;node(x,375,240,70,`候选 ${i+1}`,'',C.mint,s>=1);arrow([x+120,450],[1400,535],C.mint);}
    table(1140,540,520,[['e-class 10 · t','3'],['e-class 20 · A','1']],{title:'一份有序账表 · shared_ptr<const Bag>'});
    if(s>=1){pill('复制不搬动表项',1140,753,520,C.mint);}
    smallNote('合并时生成新表；旧表仍供其他候选使用。没有对所有相同内容做全局去重。');
  } else if(ch.id==='merge'){
    panelTitle('A：总成本4，已有 t=3',110,315,C.mint);panelTitle('B：总成本5，已有 t=3',720,315,C.violet);
    table(110,360,480,[['10 · t','3'],['20 · A 自身','4 − 3 = 1']],{title:'加入自身剩余开销',active:s>=1?0:-1,highlight:C.mint});
    table(720,360,480,[['10 · t','3'],['30 · B 自身','5 − 3 = 2']],{title:'加入自身剩余开销',active:s>=1?0:-1,highlight:C.violet});
    arrow([600,465],[700,465],C.muted);arrow([1212,465],[1290,465],C.cyan);
    const shown=s===0?-1:s===1?Math.min(2,Math.floor(p*3)):2;
    table(1310,360,500,[['10 · t','3'],['20 · A','1'],['30 · B','2']].map((r,i)=>i<=shown?r:['等待归并','—']),{title:'归并后的父账表',active:s===1?shown:-1,highlight:C.cyan});
    round(140,646,1030,117,19,C.panel,C.line);text('孩子自身开销 = 总成本 − 已记账贡献',655,681,31,C.text,600,'center');text('重复 e-class：保留较低贡献值，沿用 Tiger 规则',655,731,25,C.muted,400,'center');
    if(shown===2)text('合计 6',1560,690,67,C.mint,650,'center');text('共享的 t 只付一次',1560,761,26,C.muted,400,'center');
    smallNote('有序归并减少散乱访问与分配；缓存表内总额，避免反复求和。');
  } else if(ch.id==='evidence'){
    const phase=clamp((t-.4)/2);round(108,328,820,390,25,C.panel,C.line);round(992,328,820,390,25,C.panel,C.line);
    panelTitle('全局成本估计',140,372,C.cyan);panelTitle('整个 Tiger 提取进程',1025,372,C.mint);
    [[140,460,407.059,407.059,29.404,'ms',C.cyan],[1025,460,2.271746,2.271746,1.735388,'s',C.mint]].forEach(([x,y,max,a,b,unit,color])=>{
      text('原版',x,y-22,24,C.muted);round(x,y,720*ease(phase),38,8,'#43516a');text(`${unit==='ms'?Math.round(a):a.toFixed(3)} ${unit}`,x+720,y+64,30,C.text,600,'right');
      text('共享账表',x,y+106,24,color);round(x,y+126,720*b/max*ease(phase),38,8,color);text(`${unit==='ms'?Math.round(b):b.toFixed(3)} ${unit}`,x+720,y+191,35,color,700,'right');
    });
    if(s>=1){pill('完整提取输出：逐字节一致',125,763,590,C.mint);pill('CPU 表示优化的收益',750,763,440,C.cyan);pill('GPU 小批量：开销不划算',1220,763,590,C.amber);}
    smallNote('同一保存快照 · 每条路径单次运行 · 不把未修改阶段的波动计为 marker 收益。');
  } else if(ch.id==='close'){
    text('合法路径',110,411,77,C.violet,650);text('＋',483,411,65,C.muted);text('共享成本',610,411,77,C.mint,650);text('＝',1001,411,65,C.muted);text('同样的答案',1128,411,77,C.cyan,650);
    round(110,553,1700,145,23,C.panel,C.line);text('Statewalk 保证依赖相容；共享账表减少表示开销。',960,595,38,C.text,600,'center');text('尚未接入 matching marker / RipSatCell；尚未完成在线 extraction。',960,653,27,C.muted,400,'center');
    text('实现依据：Tiger ae80f82a  ·  账表优化 f9a6d1c',110,772,22,C.muted);text('中文合成配音 · 本地离线制作',1810,772,22,C.muted,400,'right');
  }
}
function wrap(s,maxWidth,size=34){g.font=`400 ${size}px ${font}`;let rows=[],line='';for(const c of s){if(g.measureText(line+c).width>maxWidth && line){rows.push(line);line='';}line+=c;}if(line)rows.push(line);return rows;}
function paint(frame){
  background(frame);let i=film.chapters.findIndex(c=>frame>=c.start && frame<c.start+c.frames);if(i<0)i=film.chapters.length-1;let ch=film.chapters[i],local=frame-ch.start;
  let alpha=Math.min(ease(local/(FPS*.45)),clamp((ch.frames-local)/(FPS*.3)));
  g.save();g.globalAlpha=alpha;g.translate(0,10*(1-ease(local/(FPS*.5))));header(ch,i,frame);scene(ch,frame);g.restore();
  const clip=film.clips.find(c=>frame>=c.start && frame<c.start+c.frames);
  round(76,895,1768,142,23,'#060d18e8','#23354a');
  if(clip){let rows=wrap(clip.text,1620,35);if(rows.length>2)throw Error('caption overflow: '+clip.text);rows.forEach((row,j)=>text(row,960,rows.length===1?965:940+j*49,35,C.text,400,'center'));}
  else text('STATEWALK · 从合法路径到共享成本',960,964,24,C.muted,400,'center');
  line([[0,1067],[W,1067]],'#26384b',3);line([[0,1067],[W*frame/film.frames,1067]],C.cyan,4);
  for(const c of film.chapters)g.fillRect(W*c.start/film.frames,1060,2,14);
}
function stills(){
  const dir=path.join(out,'stills');fs.mkdirSync(dir,{recursive:true});
  film.chapters.forEach((ch,i)=>{paint(ch.start+Math.round(ch.frames*.66));fs.writeFileSync(path.join(dir,`${String(i+1).padStart(2,'0')}-${ch.id}.png`),canvas.toBuffer('image/png'));});
  paint(Math.round(film.chapters[0].frames*.5));fs.writeFileSync(path.join(out,'poster.jpg'),canvas.toBuffer('image/jpeg',90));
  const height=Math.ceil(film.chapters.length/3)*360;const sheet=createCanvas(1920,height),s=sheet.getContext('2d');s.fillStyle=C.bg;s.fillRect(0,0,1920,height);
  film.chapters.forEach((ch,i)=>{paint(ch.start+Math.round(ch.frames*.66));s.drawImage(canvas,(i%3)*640,Math.floor(i/3)*360,640,360);});
  fs.writeFileSync(path.join(out,'contact-sheet.jpg'),sheet.toBuffer('image/jpeg',90));
}
async function main(){
  stills();if(preview){console.log('Preview frames ready');return;}
  const target=path.join(out,'silent.mp4'),log=fs.openSync(path.join(out,'render.log'),'w');
  const ff=spawn('ffmpeg',['-hide_banner','-y','-f','rawvideo','-pixel_format','rgba','-video_size',`${W}x${H}`,'-framerate',String(FPS),'-i','pipe:0','-an','-c:v','libx264','-preset','fast','-crf','19','-threads','6','-pix_fmt','yuv420p',target],{stdio:['pipe','ignore',log]});
  ff.stdin.on('error',e=>{console.error(e.message);process.exitCode=1;});
  for(let frame=0;frame<film.frames;frame++){
    paint(frame);if(!ff.stdin.write(canvas.data()))await once(ff.stdin,'drain');
    if(frame%480===0)console.log(`${frame}/${film.frames} frames (${Math.round(frame/film.frames*100)}%)`);
  }
  ff.stdin.end();let [code]=await once(ff,'close');fs.closeSync(log);if(code)throw Error('ffmpeg failed: '+code);console.log(target);
}
main().catch(e=>{console.error(e);process.exitCode=1;});
