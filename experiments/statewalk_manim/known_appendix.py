"""Trace-backed, stepwise explanation of known propagation and liveness projection."""
from manim import *
import json
from film import StatewalkFilm, zh, math as formula, DATA
from details import node, link, codeblock
from closure_appendix import card

BLUE='#58C4DD';GREEN='#83C167';PURPLE='#B189F7';GOLD='#FFFF00';RED='#FC6255';DIM='#646775'


def number_label(s,at,color=WHITE,size=26):
    return Text(s,font='Menlo',font_size=size,color=color).move_to(at)


def cells(values,at,columns=7,color=GREEN):
    group=VGroup()
    for i,value in enumerate(values):
        box=RoundedRectangle(width=.64,height=.39,corner_radius=.05,color=color,fill_color=color,fill_opacity=.08)
        text=Text(str(value),font='Menlo',font_size=18,color=color)
        group.add(VGroup(box,text).move_to([(i%columns)*.77,-(i//columns)*.52,0]))
    return group.move_to(at)


class KnownAppendix(StatewalkFilm):
    def construct(self):
        self.ev=json.loads((DATA/'known-evidence.json').read_text())
        assert self.ev['native_liveness_masks_equal'] and self.ev['validated_closures']==1465
        self.intro_known()
        self.closure_step()
        self.projection()
        self.future_need()
        self.late_projection()
        self.state_keys()
        self.native_summary()
        self.play(*[FadeOut(m) for m in list(self.mobjects) if m is not self.watermark],run_time=1)
        self.wait(.5);self.finish_edit()

    def intro_known(self):
        self.section('拆解一：这两行分别做什么')
        self.heading('加入 → 传播 → 筛选')
        code=codeblock(['known = pure_closure(q.known | {n.eclass})',
                        'key = (n.eclass, live_known(n.eclass, known))'],25,.75).move_to([0,1,0])
        if code.width>12.5:code.scale_to_fit_width(12.5)
        labels=VGroup(zh('新候选的结果',27,PURPLE),zh('所有可提取结果',27,GREEN),zh('用于状态判等的信息',27,GOLD)).arrange(RIGHT,buff=.65).move_to([0,-1.2,0])
        note=zh('教学伪代码 · 原生是位集、依赖计数与哈希',23,GREY_B).move_to([0,-2.45,0])
        self.beat('known_intro',lambda:self.play(FadeIn(code),FadeIn(labels),FadeIn(note),run_time=1.5))
        self.shot('40-known-two-lines')

    def closure_step(self):
        self.section('拆解二：真实 #73 → #200，逐个点亮')
        title=self.heading('先看 q，再看候选 n')
        old=card('q = #73\nposition = c9\nknown: 52 classes',[-3.6,.65,0],BLUE,29,width=5.7)
        selected=card('c9, c35, c45,\nc55, c67, c71, ...',[3.4,.65,0],GREEN,27,width=5.8)
        foot=zh('摘录关键成员；known 记录可提取性，不是实际运行值',23,GREY_B).move_to([0,-1.7,0])
        self.beat('known_before',lambda:self.play(FadeIn(old),FadeIn(selected),FadeIn(foot),run_time=1.2))
        gate=card('n = Load b1\neclass = c21',[3.4,.7,0],PURPLE,30,width=4.8)
        inputs=VGroup(*[node(t,[-3.6,y,0],GREEN,25) for t,y in [('Load op: c45',1.65),('address: c55',.45),('state: c9',-.75)]])
        arrows=VGroup(*[link(x,gate,GREEN) for x in inputs])
        ready=zh('所有输入已在 q.known 中 → 候选合法',28,GREEN).move_to([0,-2.3,0])
        def check():
            self.play(FadeOut(old),FadeOut(selected),FadeOut(foot),FadeIn(gate),FadeIn(inputs),Create(arrows),FadeIn(ready),run_time=1.5)
        self.beat('known_guard',check);self.shot('41-candidate-guard')
        expression=codeblock(['seed = q.known | {21}'],32).move_to([0,1.4,0])
        count=number_label('52 + 1 = 53',[0,.05,0],GREEN,46)
        note=zh('这里的 | 是集合并集，不是 egglog 的 union',30,GOLD).move_to([0,-1.35,0])
        parent=zh('旧记录 #73 的集合仍是 52 项',23,BLUE).move_to([0,-2.35,0])
        def seed():
            self.play(FadeOut(gate),FadeOut(inputs),FadeOut(arrows),FadeOut(ready),Transform(title,zh('只加入一个新结果类：c21',35).move_to(title)),run_time=.6)
            self.play(FadeIn(expression),Write(count),FadeIn(note),FadeIn(parent),run_time=1.4)
        self.beat('known_seed',seed);self.shot('42-seed-is-set-union')
        load=node('Load:c21',[-4.65,.4,0],PURPLE,25)
        get=node('Get:c72',[-1.25,.4,0],DIM,25)
        add=node('FAdd:c37',[3.25,.4,0],DIM,25)
        idx=node('index 0:c35',[-1.25,1.9,0],GREEN,23)
        op=node('FAdd op:c67',[3.25,1.9,0],GREEN,23)
        a=node('a1:c71',[3.25,-1.1,0],GREEN,25)
        edges=VGroup(link(load,get),link(idx,get,GREEN),link(get,add),link(op,add,GREEN),link(a,add,GREEN))
        missing_get=number_label('missing: 1',[-1.25,-.6,0],GOLD,23)
        missing_add=number_label('missing: 1',[5.5,.4,0],GOLD,21)
        total=number_label('known: 53',[-4.7,-1.25,0],GREEN,25)
        note2=zh('显示逻辑缺口数；原生计数器另有紧凑编码',22,GREY_B).move_to([0,-2.45,0])
        def get_ready():
            self.play(FadeOut(expression),FadeOut(count),FadeOut(note),FadeOut(parent),Transform(title,zh('第一波：Get 最后一个缺口补上',35).move_to(title)),run_time=.6)
            self.play(FadeIn(VGroup(load,get,add,idx,op,a,missing_get,missing_add,total,note2)),Create(edges),run_time=1)
            self.play(Indicate(load,color=PURPLE),edges[0].animate.set_color(GREEN),run_time=.7)
            self.play(get.animate.set_color(GREEN),Transform(missing_get,number_label('missing: 0',missing_get.get_center(),GREEN,23)),Transform(total,number_label('known: 54',total.get_center(),GREEN,25)),run_time=.8)
        self.beat('known_get',get_ready);self.shot('43-get-ready')
        def add_ready():
            self.play(Transform(title,zh('第二波：已有 a1，现在也有 b1',35).move_to(title)),run_time=.5)
            self.play(Indicate(a,color=GREEN),Indicate(op,color=GREEN),edges[2].animate.set_color(GREEN),run_time=.8)
            self.play(add.animate.set_color(GREEN),Transform(missing_add,number_label('missing: 0',missing_add.get_center(),GREEN,21)),Transform(total,number_label('known: 55',total.get_center(),GREEN,25)),run_time=.8)
        self.beat('known_add',add_ready);self.shot('44-add-ready')
        self.section('拆解三：一直传播，到没有新项')
        waves_title=self.heading('完整的四波，集合与原生记录一致')
        waves=self.ev['closure']['waves'];assert list(map(len,waves))==[1,4,3,2]
        boxes=VGroup()
        for i,wave in enumerate(waves):
            lines=', '.join(map(str,wave)) if i!=1 else '37, 88\n91, 101'
            at=[-5.1+i*3.4,.5,0]
            box=card(lines,at,GREEN,27,width=2.6)
            label=zh(f'第 {i+1} 波：+{len(wave)}',25,WHITE).next_to(box,UP,buff=.5)
            boxes.add(VGroup(box,label))
        links=VGroup(*[link(boxes[i][0],boxes[i+1][0],GREEN) for i in range(3)])
        equation=formula(r'52+\underbrace{1}_{\mathrm{seed}}+1+4+3+2=63',size=40,color=GOLD).move_to([0,-1.7,0])
        note=zh('按依赖分波讲解；不声称原生队列采用同一弹出顺序',22,GREY_B).move_to([0,-2.5,0])
        self.beat('known_waves',lambda:self.play(LaggedStart(*[FadeIn(x) for x in boxes],lag_ratio=.25),Create(links),Write(equation),FadeIn(note),run_time=2.1));self.shot('45-real-closure-waves')
        code=codeblock(['queue = [newly_known_class]',
                        'while queue:',
                        '    c = queue.pop()',
                        '    for n in pure_parents[c]:',
                        '        ready = all_inputs_ready(n)',
                        '        fresh = n.eclass not in known',
                        '        if ready and fresh:',
                        '            known.add(n.eclass)',
                        '            queue.append(n.eclass)'],23,.38).move_to([0,.45,0])
        if code.width>12.4:code.scale_to_fit_width(12.4)
        label=zh('固定候选图；每个类首次就绪时继续传播；不自动选择 effect',24,PURPLE).move_to([0,-2.6,0])
        self.beat('known_fixedpoint',lambda:self.play(FadeOut(boxes),FadeOut(links),FadeOut(equation),FadeOut(note),FadeIn(code),FadeIn(label),Transform(waves_title,zh('只访问新增项的纯父节点',35).move_to(waves_title)),run_time=1.5));self.shot('46-worklist-not-rule-execution')

    def projection(self):
        self.section('拆解四：known 与 live_known 各留什么')
        self.heading('完整记录，和用于比较的投影')
        left=card('known\n63 classes',[-3.4,.5,0],GREEN,37,width=4.5)
        right=card('key\nposition + live bits',[3.25,.5,0],GOLD,28,width=5.3)
        connection=link(left,right,GOLD)
        foot=zh('从左边筛出一份比较信息；左边仍然保留',29,WHITE).move_to([0,-1.6,0])
        self.beat('known_projection',lambda:self.play(FadeIn(left),FadeIn(right),Create(connection),FadeIn(foot),run_time=1.5))
        groups=VGroup(card('45\ninitial',[-4.7,.65,0],GREY_B,31,width=3.3),card('4\neffect',[-.5,.65,0],PURPLE,31,width=3.1),card('14\ndynamic pure',[4,.65,0],GREEN,29,width=4))
        key=codeblock(['key = (c21, 14 relevant pure bits)'],27).move_to([0,-1.3,0])
        note=zh('这 14 项目前全部仍可能有用；不是“49 项已经死了”',25,GOLD).move_to([0,-2.3,0])
        self.beat('known_initial',lambda:self.play(FadeOut(left),FadeOut(right),FadeOut(connection),FadeOut(foot),FadeIn(groups),FadeIn(key),FadeIn(note),run_time=1.5));self.shot('47-known-breakdown')

    def future_need(self):
        self.section('拆解五：未来需求从哪里来')
        self.heading('向前看 effect，再向后找 pure 依赖')
        pos=node('position',[-5.1,0,0],PURPLE,24)
        w1=node('Write 1',[-1.9,.65,0],PURPLE,24);w2=node('Write 2',[2.15,.65,0],PURPLE,24)
        branch=node('Other effect',[-.8,-1.35,0],PURPLE,24)
        x=node('input x',[-1.9,2.05,0],DIM,22);m=node('f(y)',[2.15,2.05,0],DIM,23);y=node('input y',[5.2,2.05,0],DIM,22)
        z=node('input z',[3.4,-1.35,0],DIM,22)
        links=VGroup(link(pos,w1,PURPLE),link(w1,w2,PURPLE),link(pos,branch,PURPLE),link(x,w1),link(m,w2),link(y,m),link(z,branch))
        note=zh('固定 region 的候选图：包含远处与备选路径',24,GREY_B).move_to([0,-2.55,0])
        def scan():
            self.play(FadeIn(VGroup(pos,w1,w2,branch,x,m,y,z)),Create(links),FadeIn(note),run_time=1.2)
            self.play(Indicate(w1,color=PURPLE),Indicate(w2,color=PURPLE),Indicate(branch,color=PURPLE),run_time=.9)
            self.play(*[obj.animate.set_color(GREEN) for obj in [x,m,y,z]],*[edge.animate.set_color(GREEN) for edge in links[3:]],run_time=1.1)
        self.beat('live_future',scan);self.shot('48-live-future-dependencies')

    def late_projection(self):
        self.section('拆解六：真实 #798，41 项变成 16 项判等')
        self.heading('写入之后，哪些区别已经不必再记入键？')
        r=self.ev['records']['798'];assert len(r['dynamic_pure'])==41 and len(r['live_known'])==16
        left=cells(r['dynamic_pure'],[-3.4,.3,0],7,GREEN)
        right=cells(r['live_known'],[3.55,.3,0],4,GOLD)
        titles=VGroup(zh('完整记录中的 41 项动态 pure',25,GREEN).move_to([-3.4,2.35,0]),zh('用于判等的 16 项',25,GOLD).move_to([3.55,2.35,0]))
        note=zh('真实位置 c5（Write） · 总 known = 45 初始 + 10 effect + 41 pure = 96',21,GREY_B).move_to([0,-2.3,0])
        selection=VGroup(*[left[r['dynamic_pure'].index(c)] for c in r['live_known']])
        def select():
            self.play(FadeIn(left),FadeIn(titles),FadeIn(note),run_time=1)
            self.play(*[left[i].animate.set_color(GOLD if c in r['live_known'] else GREY_B) for i,c in enumerate(r['dynamic_pure'])],run_time=1)
            self.play(TransformFromCopy(selection,right),run_time=1.6)
        self.beat('live_late',select);self.shot('49-live-filter-real')
        full=SurroundingRectangle(left,buff=.18,color=GREEN)
        text=zh('25 项仍在完整记录中；只是不参与这个位置的判等',27,GREEN).move_to([0,-2.3,0])
        self.beat('live_not_delete',lambda:self.play(Create(full),Transform(note,text),Indicate(left,color=GREEN),run_time=1.3));self.shot('50-projection-not-deletion')

    def state_keys(self):
        self.section('拆解七：什么时候两条记录可以共用一个键')
        self.heading('比较同一位置的相关信息，再比较成本')
        first=card('pos = P, cost = 10\nknown: need1, old_A',[-3.4,1.05,0],BLUE,27,width=5.8)
        second=card('pos = P, cost = 12\nknown: need1, old_B',[3.4,1.05,0],PURPLE,27,width=5.8)
        key=card('key = (P, {need1})',[0,-.9,0],GOLD,31,width=5.6)
        edges=VGroup(Arrow(first.get_bottom(),key.get_top()+LEFT*1.1,buff=.1,color=BLUE),
                     Arrow(second.get_bottom(),key.get_top()+RIGHT*1.1,buff=.1,color=PURPLE))
        note=zh('教学例子：后续不需要 old_A / old_B；保留 cost=10 的到达方式',22,GREY_B).move_to([0,-2.4,0])
        self.beat('live_same_key',lambda:self.play(FadeIn(first),FadeIn(second),FadeIn(key),Create(edges),FadeIn(note),Circumscribe(first,color=GREEN),run_time=1.8))
        self.shot('51-same-relevant-state')
        next_first=card('pos = P\nknown: need1, need2',first.get_center(),BLUE,27,width=5.8)
        next_second=card('pos = P\nknown: need1',second.get_center(),PURPLE,27,width=5.8)
        different=zh('未来可能需要 need2 → 两个 key 必须不同',30,RED).move_to([0,-1.1,0])
        disclaimer=zh('多保留一些差异只是多搜索；漏掉关键差异可能错误剪枝',23,GREY_B).move_to([0,-2.4,0])
        self.beat('live_missing',lambda:self.play(Transform(first,next_first),Transform(second,next_second),FadeOut(key),FadeOut(edges),FadeIn(different),Transform(note,disclaimer),run_time=1.5));self.shot('52-keep-future-differences')

    def native_summary(self):
        self.section('拆解总结：完整集合留下，投影用于判等')
        title=self.heading('教学集合写法，对应原生的不同存储')
        rows=VGroup(*[VGroup(Text(a,font='Menlo',font_size=27,color=GREEN),zh(b,25,c)).arrange(RIGHT,buff=.65) for a,b,c in [
            ('known','完整位集 + 公共初始信息',GREEN),('live_pure[position]','预先计算未来相关的 pure 类',BLUE),('key','当前位置 + masked_hash',GOLD)]]).arrange(DOWN,buff=.6).move_to([0,.55,0])
        note=zh('集合含义 ≠ 无碰撞哈希保证；这里没有成本 bag 的累加',24,GREY_B).move_to([0,-2,0])
        self.beat('known_native',lambda:self.play(FadeIn(rows),FadeIn(note),run_time=1.5));self.shot('53-native-data-structures')
        code=codeblock(['assert all_inputs_ready(n, q.known)',
                        'known = pure_closure(q.known | {n.eclass})',
                        'live = (known - initial_known) & live_pure[n.eclass]',
                        'key = (n.eclass, live)',
                        'remember_if_cheaper(key, known, cost, parent=q)'],24,.6).move_to([0,.4,0])
        if code.width>12.4:code.scale_to_fit_width(12.4)
        finish=zh('known：知道什么     live_known：还需要区分什么',27,GOLD).move_to([0,-2.35,0])
        self.beat('known_finish',lambda:self.play(FadeOut(rows),FadeOut(note),FadeIn(code),FadeIn(finish),run_time=1.5));self.shot('54-known-two-lines-expanded')
