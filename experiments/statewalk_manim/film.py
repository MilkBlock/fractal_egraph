"""Manim Community: continuous visual explanation driven by verified native data."""
from manim import *
import json
import os
from pathlib import Path
import numpy as np
from details import DetailScenes

config.background_color = '#101014'
config.notify_outdated_version = False
FONT = 'PingFang SC'
DATA = Path(os.environ.get('STATEWALK_MANIM_DIR','out/statewalk-manim')).resolve()
BLUE_VALUE = '#58C4DD'
GREEN_READY = '#83C167'
PURPLE_STATE = '#B189F7'
GOLD_COST = '#FFFF00'
RED_MISSING = '#FC6255'
DIM = '#646775'


def zh(s,size=30,color=WHITE):
    return Text(s,font=FONT,font_size=size,color=color)

def math(*parts,size=40,color=WHITE):
    return MathTex(*parts,font_size=size,color=color)

def token(label,color=BLUE_VALUE,r=.32):
    return VGroup(Circle(radius=r,color=color,stroke_width=2,fill_color=color,fill_opacity=.08),math(label,size=29,color=color))

def arrow(a,b,color=GREY_B):
    aa=a.get_center() if isinstance(a,Mobject) else np.array(a);bb=b.get_center() if isinstance(b,Mobject) else np.array(b)
    direction=bb-aa;direction=direction/np.linalg.norm(direction)
    start=a.get_boundary_point(direction) if isinstance(a,Mobject) else aa
    end=b.get_boundary_point(-direction) if isinstance(b,Mobject) else bb
    return Arrow(start,end,buff=.09,color=color,stroke_width=2,tip_length=.15,max_tip_length_to_length_ratio=.12)

def dim_token(obj):
    obj[0].set_stroke(opacity=.38).set_fill(opacity=.02);obj[1].set_opacity(.42);return obj

class Bag(VGroup):
    def __init__(self,entries,color=BLUE_VALUE):
        self.entries=VGroup(*[math(k,r'\mapsto',str(v),size=34,color=color) for k,v in entries]).arrange(DOWN,buff=.26,aligned_edge=LEFT)
        x=self.entries.get_left()[0]-.24;y0=self.entries.get_bottom()[1]-.18;y1=self.entries.get_top()[1]+.18
        self.brackets=VGroup(Line([x+.12,y1,0],[x,y1,0]),Line([x,y1,0],[x,y0,0]),Line([x,y0,0],[x+.12,y0,0]))
        self.brackets.set_color(color)
        super().__init__(self.entries,self.brackets)

class StatewalkFilm(DetailScenes,Scene):
    def setup(self):
        audio_data=json.loads((DATA/'audio.json').read_text())
        self.audio=audio_data['clips'];self.voice_label=audio_data.get('narration_label','macOS Tingting（合成语音）')
        self.trace=json.loads((DATA/'trace-demo.json').read_text())
        self.ir=json.loads((DATA/'ir-demo.json').read_text())
        self.flow=json.loads((DATA/'dataflow.json').read_text())
        assert [v['label'] for v in self.ir['loads']]==['b[0]','a[1]','b[1]','a[2]','b[2]','a[0]']
        assert len(self.trace['state_graph'])==808 and self.trace['summary']['closure_cache_hits']==0
        assert self.trace['rejection']['from']==71 and self.trace['accepted']['id']==797
        self.caption=None;self.captions=[];self.chapters=[]
        self.watermark=zh('Manim Community · 原创中文图解',13,color=GREY_B).to_corner(UR,buff=.20)
        self.add(self.watermark)
        (DATA/'stills').mkdir(exist_ok=True)

    def section(self,name):
        self.chapters.append(dict(title=name,start=float(self.time)))
        old=[m for m in self.mobjects if m is not self.caption and m is not self.watermark]
        if old:self.play(*[FadeOut(m) for m in old],run_time=.7)
        self.remove(*old)

    def beat(self,key,action=None):
        clip=self.audio[key];caption=zh(clip['caption'],27)
        if caption.width>12.7:caption.scale_to_fit_width(12.7)
        caption.to_edge(DOWN,buff=.22)
        animations=[FadeIn(caption,shift=UP*.08)]
        if self.caption is not None:animations.append(FadeOut(self.caption))
        self.play(*animations,run_time=.22);self.caption=caption
        start=float(self.time);self.add_sound(clip['audio'])
        if action:action()
        elapsed=float(self.time)-start
        if elapsed<clip['duration']:self.wait(clip['duration']-elapsed,frozen_frame=True)
        self.wait(.3,frozen_frame=True)
        self.captions.append(dict(key=key,start=start,end=float(self.time),text=clip['subtitle']))

    def shot(self,name):
        self.renderer.update_frame(self,ignore_skipping=True)
        self.renderer.camera.get_image().save(DATA/'stills'/f'{name}.png')

    def heading(self,s):
        obj=zh(s,35).to_edge(UP,buff=.52)
        if obj.width>12.5:obj.scale_to_fit_width(12.5)
        self.play(FadeIn(obj,shift=DOWN*.12),run_time=.6)
        return obj

    def construct(self):
        self.intro()
        self.tuple_demo()
        self.availability()
        self.product_state()
        self.closure_detail()
        self.gate()
        self.relaxation_detail()
        self.search()
        self.unroll_detail()
        self.fill_dag_detail()
        self.python_detail()
        self.sharing()
        self.storage()
        self.results()
        self.finish_edit()

    def finish_edit(self):
        for i,c in enumerate(self.chapters):c['end']=self.chapters[i+1]['start'] if i+1<len(self.chapters) else float(self.time)
        (DATA/'edit.json').write_text(json.dumps(dict(duration=float(self.time),chapters=self.chapters,captions=self.captions,renderer='Manim Community 0.19.0',width=config.pixel_width,height=config.pixel_height,fps=config.frame_rate),ensure_ascii=False,indent=2)+'\n')

    def intro(self):
        self.section('从三个加法开始')
        title=Text('Statewalk',font='STIX Two Text',slant=ITALIC,font_size=74,color=BLUE_VALUE).shift(UP*1.6)
        formula=math(r'\operatorname{vec\_add}(a,b)', '=', r'[a_0+b_0,\ a_1+b_1,\ a_2+b_2]',size=42).shift(DOWN*.15)
        note=zh('raytrace.rs · 真实 eggcc IR',23,color=GREY_B).next_to(formula,DOWN,buff=.6)
        self.beat('opening',lambda:self.play(Write(title),Write(formula),run_time=2.6))
        self.beat('source',lambda:self.play(FadeIn(note),Circumscribe(formula[2],color=GOLD_COST),run_time=1.5))
        self.shot('01-opening')

    def availability(self):
        self.section('像钥匙一样，逐个获得可用值')
        self.heading('一个加号，要等两把“钥匙”')
        self.inputs={};self.outputs=[];self.pluses=[];wires=VGroup()
        for i,y in enumerate([1.8,.2,-1.4]):
            a=token(f'a_{i}',BLUE_VALUE).move_to([-4.9,y+.36,0]);b=token(f'b_{i}',TEAL_A).move_to([-4.9,y-.36,0])
            dim_token(a);dim_token(b);self.inputs[f'a[{i}]']=a;self.inputs[f'b[{i}]']=b
            plus=token('+',GREY_B,r=.30).move_to([-1.4,y,0]);dim_token(plus)
            result=math(f'a_{i}+b_{i}',size=36,color=GREY_C).move_to([1.3,y,0]);result.set_opacity(.50)
            wires.add(arrow(a,plus,DIM),arrow(b,plus,DIM),arrow(plus,result,DIM));self.pluses.append(plus);self.outputs.append(result)
        self.cursor=token('c9',PURPLE_STATE,r=.47).move_to([4.6,-1.4,0])
        load=zh('Load',31,color=PURPLE_STATE).move_to([4.6,1.35,0])
        current=math('b_0',size=33,color=BLUE_VALUE).next_to(load,DOWN,buff=.25)
        lane1=ArcBetweenPoints(self.cursor.get_top(),load.get_bottom(),angle=-PI/2,color=PURPLE_STATE)
        lane2=ArcBetweenPoints(load.get_bottom(),self.cursor.get_top(),angle=-PI/2,color=PURPLE_STATE)
        loop=VGroup(lane1,lane2);moving=Dot(self.cursor.get_top(),radius=.065,color=GOLD_COST)
        legend=zh('亮起：可提取     灰色：依赖还没齐',23,color=GREY_B).move_to([-2,-2.5,0])
        self.beat('keys',lambda:self.play(FadeIn(VGroup(*self.inputs.values(),*self.pluses,*self.outputs,wires,self.cursor,load,current,loop,legend)),run_time=1.4))
        ready=set();lit=set()
        def read(key):
            nonlocal current
            replacement=math(key[0]+'_'+key[2],size=33,color=BLUE_VALUE).move_to(current)
            self.play(Transform(current,replacement),Indicate(self.inputs[key],color=GOLD_COST),run_time=.4)
            self.add(moving);moving.move_to(lane1.get_start())
            self.play(MoveAlongPath(moving,lane1),run_time=.48)
            self.play(self.inputs[key][0].animate.set_stroke(opacity=1).set_fill(opacity=.08),self.inputs[key][1].animate.set_opacity(1),Flash(self.inputs[key],color=BLUE_VALUE,line_length=.13),run_time=.45)
            self.play(MoveAlongPath(moving,lane2),run_time=.48);self.remove(moving)
            ready.add(key)
            for i in range(3):
                if f'a[{i}]' in ready and f'b[{i}]' in ready and i not in lit:
                    lit.add(i)
                    self.play(self.pluses[i][0].animate.set_stroke(color=GREEN_READY,opacity=1).set_fill(color=GREEN_READY,opacity=.08),self.pluses[i][1].animate.set_opacity(1).set_color(GREEN_READY),self.outputs[i].animate.set_opacity(1).set_color(GREEN_READY),run_time=.45)
        self.beat('first_read',lambda:read('b[0]'))
        self.beat('middle_read',lambda:[read(k) for k in ['a[1]','b[1]']])
        self.shot('02-middle-ready')
        self.beat('remaining_read',lambda:[read(k) for k in ['a[2]','b[2]','a[0]']])
        self.beat('return_state',lambda:self.play(Circumscribe(self.cursor,color=GOLD_COST),run_time=1.3))
        self.shot('03-all-ready')

    def tuple_demo(self):
        self.section('真实IR：一次读取分出两条线')
        self.heading('同一次读取，给出两样东西')
        code=Text('(Bop (Load) ptr state)',font='Menlo',font_size=34,color=WHITE).move_to([0,2.05,0])
        origin=zh(f'真实节点 {self.ir["loads"][0]["node"]} · 显示变量已改名',20,color=GREY_B).next_to(code,UP,buff=.25)
        pair=math('(',r'\mathrm{value}',',',r'\mathrm{state}',')',size=48).move_to([0,.6,0]);pair[1].set_color(BLUE_VALUE);pair[3].set_color(PURPLE_STATE)
        value=math('b_0',size=55,color=BLUE_VALUE).move_to([-3,-1.1,0]);state=math("s'",size=55,color=PURPLE_STATE).move_to([3,-1.1,0])
        get0=Text('Get(load, 0)',font='Menlo',font_size=25,color=BLUE_VALUE).next_to(value,DOWN,buff=.35)
        get1=Text('Get(load, 1)',font='Menlo',font_size=25,color=PURPLE_STATE).next_to(state,DOWN,buff=.35)
        def split():
            self.play(Write(code),FadeIn(origin),run_time=1)
            self.play(Write(pair),run_time=.8)
            self.play(Create(arrow(pair[1],value,BLUE_VALUE)),Create(arrow(pair[3],state,PURPLE_STATE)),TransformFromCopy(pair[1],value),TransformFromCopy(pair[3],state),FadeIn(get0),FadeIn(get1),run_time=1.8)
        self.beat('tuple',split);self.shot('01b-real-ir')

    def product_state(self):
        self.section('把位置与知识一起展开')
        self.heading('同一个位置，带着不同的“钥匙”')
        center=token('c9',PURPLE_STATE,r=.55).move_to([0,1.2,0]);self.play(FadeIn(center),run_time=.6)
        left=center.copy().move_to([-3.4,1.2,0]);right=center.copy().move_to([3.4,1.2,0])
        left_keys=math(r'\{b_0\}',size=47,color=BLUE_VALUE).move_to([-3.4,-.25,0])
        right_keys=math(r'\{b_0,a_1\}',size=47,color=GREEN_READY).move_to([3.4,-.25,0])
        labels=VGroup(zh('#11 · 估价613',24,color=GREY_B).move_to([-3.4,2.2,0]),zh('#73 · 估价1225',24,color=GREY_B).move_to([3.4,2.2,0]))
        def split():
            self.play(TransformFromCopy(center,left),Transform(center,right),run_time=1.3)
            self.play(FadeIn(left_keys,shift=UP*.2),FadeIn(right_keys,shift=UP*.2),FadeIn(labels),run_time=.8)
        self.beat('split_state',split)
        eq=VGroup(zh('搜索状态',31),math('=',size=38),zh('当前位置',31,color=PURPLE_STATE),math(r'\times',size=38),zh('可提取信息',31,color=GREEN_READY)).arrange(RIGHT,buff=.28).move_to([0,-1.8,0])
        self.beat('product_state',lambda:self.play(Write(eq),Indicate(right_keys[0],color=GOLD_COST),run_time=1.8))
        self.shot('04-product-state')

    def gate(self):
        self.section('三把钥匙才能打开写入')
        self.heading('真实失败：写入的数值还没有来源')
        labels=['地址 p','数值 r₀','状态 s'];colors=[BLUE_VALUE,RED_MISSING,PURPLE_STATE];ports=VGroup();wires=VGroup()
        for i,y in enumerate([1.55,.2,-1.15]):
            dot=Circle(radius=.38,color=colors[i],fill_opacity=.1,fill_color=colors[i]).move_to([-4.7,y,0])
            label=zh(labels[i],28,color=colors[i]).next_to(dot,LEFT,buff=.35)
            check=zh('✓' if i!=1 else '?',30,color=colors[i]).move_to(dot)
            ports.add(VGroup(dot,label,check));wires.add(Arrow(dot.get_right(),[1.25,.55+(1-i)*.35,0],buff=.08,color=colors[i],stroke_width=2))
        op=VGroup(RoundedRectangle(width=2.05,height=1.65,corner_radius=.13,color=GREY_B),zh('Write',37)).move_to([2.35,.55,0])
        exit_node=token('c5',PURPLE_STATE,r=.45).move_to([5.8,.55,0]);dim_token(exit_node)
        exit_edge=arrow(op,exit_node,DIM)
        door=Line([4.0,-.22,0],[4.0,1.32,0],color=RED_MISSING,stroke_width=6)
        record=zh('记录 #71',28,color=GREY_B).move_to([-3.5,2.5,0])
        missing=math(r'r_0=a_0+b_0',size=35,color=RED_MISSING).move_to([-1.35,-1.75,0])
        self.beat('write_gate',lambda:self.play(FadeIn(ports),Create(wires),FadeIn(op),FadeIn(exit_node),Create(exit_edge),Create(door),FadeIn(record),run_time=1.5))
        bead=Dot([3.4,.55,0],radius=.09,color=GOLD_COST)
        def block():
            self.add(bead);self.play(bead.animate.move_to([3.85,.55,0]),run_time=.55)
            self.play(Wiggle(door),Indicate(ports[1],color=RED_MISSING),Write(missing),run_time=1.3)
        self.beat('blocked',block);self.shot('05-blocked')
        def open_gate():
            self.play(Transform(record,zh('另一条记录 #797',28,color=GREEN_READY).move_to(record)),ports[1].animate.set_color(GREEN_READY),wires[1].animate.set_color(GREEN_READY),Transform(ports[1][2],zh('✓',30,color=GREEN_READY).move_to(ports[1][2])),run_time=1.0)
            self.play(FadeOut(door,shift=UP),FadeOut(missing),exit_edge.animate.set_color(PURPLE_STATE),exit_node[0].animate.set_stroke(opacity=1).set_fill(opacity=.08),exit_node[1].animate.set_opacity(1),run_time=.65)
            self.play(bead.animate.move_to(exit_node.get_center()),Flash(exit_node,color=PURPLE_STATE),run_time=1.0)
            self.remove(bead)
        self.beat('other_state',open_gate);self.shot('06-accepted')
        self.beat('branching',lambda:self.play(Indicate(exit_node,color=GOLD_COST),run_time=.8))

    def search(self):
        self.section('在真实搜索状态中拎出路径')
        self.heading('每个点，是一份不同的搜索状态')
        states=self.trace['state_graph'];byid={s['id']:s for s in states};depth={}
        for s in states:depth[s['id']]=0 if s['prev']<0 else depth[s['prev']]+1
        maximum=max(depth.values())
        order=['a[0]','b[0]','a[1]','b[1]','a[2]','b[2]'];points={}
        for s in states:
            mask=sum(1<<i for i,v in enumerate(order) if v in s['values'])
            points[s['id']]=np.array([-5.8+11.6*depth[s['id']]/maximum,-2.0+4*mask/63+np.sin(s['id']*17.35)*.10,0])
        edges=VGroup(*[Line(points[s['prev']],points[s['id']],stroke_width=.55,color=GREY_C,stroke_opacity=.27) for s in states if s['prev']>=0])
        dots=VGroup(*[Dot(points[s['id']],radius=.018,color=BLUE_VALUE,fill_opacity=.7) for s in states])
        label=zh('808个真实状态 · 保留的前驱关系',27,color=BLUE_VALUE).move_to([0,2.85,0])
        axis=Line([-5.8,-2.45,0],[5.8,-2.45,0],color=GREY_B,stroke_width=1)
        axis_label=zh('横向：前驱深度       纵向：不同可用信息的排布（仅用于展示）',18,color=GREY_B).next_to(axis,DOWN,buff=.13)
        self.beat('search_cloud',lambda:self.play(Create(edges),FadeIn(dots),FadeIn(label),Create(axis),FadeIn(axis_label),run_time=3.2))
        queue_dot=Dot(radius=.075,color=GOLD_COST);estimate=zh('累计估价 0',24,color=GOLD_COST).move_to([4.6,2.85,0])
        def queue():
            self.add(queue_dot,estimate)
            for e in self.trace['first_pops'][:10]:
                new=zh(f'估价 {e["cost"]}',24,color=GOLD_COST).move_to(estimate)
                self.play(queue_dot.animate.move_to(points[e['id']]),Transform(estimate,new),run_time=.48)
        self.beat('queue',queue);self.shot('07-search-cloud')
        path_points=[points[s['id']] for s in reversed(self.trace['chosen'])]
        path=VMobject(color=GOLD_COST,stroke_width=4).set_points_as_corners(path_points)
        chosen_dots=VGroup(*[Dot(p,radius=.035,color=GOLD_COST) for p in path_points])
        def trace_back():
            self.play(FadeOut(queue_dot),FadeOut(estimate),dots.animate.set_opacity(.25),edges.animate.set_opacity(.15),run_time=.6)
            self.play(Create(path),run_time=3.0,rate_func=linear);self.play(FadeIn(chosen_dots),run_time=.5)
        self.beat('backtrack',trace_back);self.shot('08-chosen-path')
        note=zh('1,864次检查 · 399次依赖未齐 · 纯闭包缓存命中0次',24,color=GREY_B).move_to([0,2.85,0])
        self.beat('cache_scope',lambda:self.play(Transform(label,note),run_time=.8))

    def sharing(self):
        self.section('同一份计算，只付一次钱')
        self.heading('同一个 t，两边都需要')
        t=token('t',BLUE_VALUE,r=.48).move_to([0,1.55,0]);a=token('A',GREEN_READY,r=.45).move_to([-3.3,-.1,0]);b=token('B',PURPLE_STATE,r=.45).move_to([3.3,-.1,0])
        wires=VGroup(arrow(t,a,BLUE_VALUE),arrow(t,b,BLUE_VALUE))
        costs=VGroup(math('3',size=40,color=GOLD_COST).next_to(t,RIGHT,buff=.22),math('1',size=40,color=GREEN_READY).next_to(a,LEFT,buff=.22),math('2',size=40,color=PURPLE_STATE).next_to(b,RIGHT,buff=.22))
        self.beat('shared_problem',lambda:self.play(FadeIn(t),FadeIn(a),FadeIn(b),Create(wires),Write(costs),run_time=1.7))
        left_coins=VGroup(*[Dot(radius=.09,color=GOLD_COST if i<3 else GREEN_READY) for i in range(4)]).arrange(RIGHT,buff=.16).move_to([-3.3,-1.4,0])
        right_coins=VGroup(*[Dot(radius=.09,color=GOLD_COST if i<3 else PURPLE_STATE) for i in range(5)]).arrange(RIGHT,buff=.16).move_to([3.3,-1.4,0])
        naive=math('4','+','5','=','9',size=49).move_to([0,-2.3,0])
        self.beat('double_count',lambda:self.play(FadeIn(left_coins),FadeIn(right_coins),Write(naive),run_time=1.7))
        combined=VGroup(*[Dot(radius=.11,color=GOLD_COST if i<3 else GREEN_READY if i==3 else PURPLE_STATE) for i in range(6)]).arrange(RIGHT,buff=.23).move_to([0,-1.4,0])
        correct=math('3','+','1','+','2','=','6',size=49).move_to(naive)
        def pay():
            self.play(Circumscribe(VGroup(*left_coins[:3]),color=RED_MISSING),Circumscribe(VGroup(*right_coins[:3]),color=RED_MISSING),run_time=1.2)
            anim=[Transform(left_coins[i],combined[i]) for i in range(4)]
            anim += [Transform(right_coins[i],combined[i]) for i in range(3)]
            anim += [Transform(right_coins[i],combined[i+1]) for i in [3,4]]
            self.play(*anim,TransformMatchingTex(naive,correct),run_time=1.5)
            self.remove(left_coins,right_coins);self.add(combined)
        self.beat('pay_once',pay);self.shot('09-pay-once')
        note=zh('必须保留：同一个 e-class 的身份',27,color=BLUE_VALUE).move_to([0,2.65,0])
        self.beat('need_ledger',lambda:self.play(FadeIn(note),Indicate(t,color=GOLD_COST),run_time=1.2))

    def storage(self):
        self.section('共享旧账本，合并产生新账本')
        title=self.heading('把“复制整张表”变成“复制指针”')
        states=VGroup(*[zh(f'候选 {i+1}',27).move_to([-4.4+i*4.4,2.05,0]) for i in range(3)])
        bags=VGroup(*[Bag([('t',3),('A',1)],BLUE_VALUE).move_to([-4.4+i*4.4,.45,0]) for i in range(3)])
        arrows=VGroup(*[arrow(states[i],bags[i],GREY_B) for i in range(3)])
        self.beat('old_copy',lambda:self.play(FadeIn(states),LaggedStart(*[FadeIn(b) for b in bags],lag_ratio=.3),Create(arrows),run_time=2.1))
        shared=Bag([('t',3),('A',1)],BLUE_VALUE).move_to([0,-.25,0]);links=VGroup(*[arrow(s,shared,BLUE_VALUE) for s in states])
        immutable=zh('不可变的旧账表',24,color=BLUE_VALUE).next_to(shared,DOWN,buff=.3)
        def share():
            self.play(FadeOut(arrows),*[Transform(b,shared.copy()) for b in bags],run_time=1.4)
            self.remove(*bags);self.add(shared);self.play(Create(links),FadeIn(immutable),run_time=.8)
        self.beat('new_share',share);self.shot('10-shared-pointer')
        left=Bag([('t',3),('A',1)],BLUE_VALUE).move_to([-4.2,.5,0]);right=Bag([('t',3),('B',2)],PURPLE_STATE).move_to([-.5,.5,0])
        output=Bag([('t',3),('A',1),('B',2)],GREEN_READY).move_to([4.25,.5,0])
        own=VGroup(math(r'A:4-3=1',size=32,color=BLUE_VALUE),math(r'B:5-3=2',size=32,color=PURPLE_STATE)).arrange(RIGHT,buff=1).move_to([0,2.15,0])
        def own_cost():
            new_title=zh('共享旧表，再归并成一张新表',35).move_to(title)
            self.play(Transform(title,new_title),FadeOut(states),FadeOut(links),FadeOut(immutable),Transform(shared,left),FadeIn(right),FadeIn(output.brackets),Write(own),run_time=1.4)
        self.beat('own_cost',own_cost)
        # shared now has left's geometry; retain its actual row objects.
        cursor_l=Triangle(color=GOLD_COST,fill_opacity=1).scale(.09).rotate(-PI/2).next_to(shared.entries[0],LEFT,buff=.18)
        cursor_r=cursor_l.copy().next_to(right.entries[0],LEFT,buff=.18)
        def merge_t():
            self.play(FadeIn(cursor_l),FadeIn(cursor_r),Circumscribe(shared.entries[0],color=GOLD_COST),Circumscribe(right.entries[0],color=GOLD_COST),run_time=1.0)
            self.play(TransformFromCopy(shared.entries[0],output.entries[0]),run_time=1.0)
        self.beat('merge_t',merge_t)
        def merge_rest():
            self.play(cursor_l.animate.next_to(shared.entries[1],LEFT,buff=.18),cursor_r.animate.next_to(right.entries[1],LEFT,buff=.18),run_time=.65)
            self.play(TransformFromCopy(shared.entries[1],output.entries[1]),run_time=.8)
            self.play(FadeOut(cursor_l),TransformFromCopy(right.entries[1],output.entries[2]),run_time=.8)
            self.play(FadeOut(cursor_r),Write(math('3+1+2=6',size=46,color=GREEN_READY).move_to([3.6,-1.55,0])),run_time=.8)
        self.beat('merge_rest',merge_rest);self.shot('11-merge')
        note=zh('旧表仍在。新表独立。选择规则不变。',28,color=GREY_B).move_to([0,-2.55,0])
        self.beat('why_faster',lambda:self.play(FadeIn(note),Indicate(VGroup(shared,right),color=BLUE_VALUE),run_time=1.2))

    def results(self):
        self.section('验证：答案保持不变')
        title=self.heading('搬运更少，答案相同')
        label=zh('全局成本估计',28,color=GREY_B).move_to([-3.8,2.1,0])
        old=Rectangle(width=9.0,height=.45,color=BLUE_VALUE,fill_opacity=.85).move_to([-0.4,1.05,0])
        new=Rectangle(width=9*29.404/407.059,height=.45,color=GREEN_READY,fill_opacity=.85).move_to(old).align_to(old,LEFT).shift(DOWN*1.1)
        old_number=math(r'407\ \mathrm{ms}',size=42,color=BLUE_VALUE).next_to(old,RIGHT,buff=.2)
        new_number=math(r'29\ \mathrm{ms}',size=42,color=GREEN_READY).next_to(new,RIGHT,buff=.25)
        whole=math(r'2.272\ \mathrm{s}\quad\longrightarrow\quad1.735\ \mathrm{s}',size=43).move_to([0,-1.3,0])
        whole_note=zh('整个 Tiger 提取进程 · 同一 raytrace Pass 3 快照 · 单次对照',23,color=GREY_B).next_to(whole,DOWN,buff=.3)
        self.beat('measurement',lambda:self.play(FadeIn(label),GrowFromEdge(old,LEFT),GrowFromEdge(new,LEFT),Write(old_number),Write(new_number),Write(whole),FadeIn(whole_note),run_time=2.1))
        check=zh('完整提取输出：逐字节一致',28,color=GREEN_READY).move_to([0,2.7,0])
        self.beat('same_answer',lambda:self.play(FadeIn(check),run_time=.8));self.shot('12-evidence')
        self.play(*[FadeOut(m) for m in [label,old,new,old_number,new_number,whole,whole_note,check]],run_time=.7)
        pieces=VGroup(zh('合法路径',40,color=PURPLE_STATE),math('+',size=45),zh('共享成本',40,color=BLUE_VALUE),math('=',size=45),zh('同样答案',40,color=GREEN_READY)).arrange(RIGHT,buff=.25).move_to([0,.6,0])
        self.beat('closing',lambda:self.play(Write(pieces),run_time=1.7))
        credits=VGroup(zh('Manim Community 0.19 · '+self.voice_label,21,color=GREY_B),zh('Tiger ae80f82a · 账表优化 f9a6d1c · 真实原生 trace',20,color=GREY_B)).arrange(DOWN,buff=.25).move_to([0,-1.3,0])
        self.beat('boundary',lambda:self.play(FadeIn(credits),run_time=.8))
        self.shot('13-end');self.play(FadeOut(pieces),FadeOut(credits),FadeOut(title),run_time=1.0);self.wait(.4)
