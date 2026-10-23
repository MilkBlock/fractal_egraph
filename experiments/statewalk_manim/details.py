"""Dependency propagation, path reconstruction and generated-code scenes."""
from manim import *
from syntax import python_lines, markup_line

BLUE = '#58C4DD'
GREEN = '#83C167'
PURPLE = '#B189F7'
GOLD = '#FFFF00'
RED = '#FC6255'


def text(s, size=28, color=WHITE):
    return Text(s, font='PingFang SC', font_size=size, color=color)


def node(s, at, color=BLUE, size=29):
    label = Text(s, font='Menlo',font_size=size,color=color).move_to(at)
    box = RoundedRectangle(width=max(1.35,label.width+.35),height=.66,
                           corner_radius=.12,color=color,fill_color=color,fill_opacity=.05).move_to(label)
    return VGroup(box,label)


def link(a,b,color=GREY_B):
    delta=b.get_center()-a.get_center()
    if abs(delta[0]) > abs(delta[1])*1.5:
        start,end=(a.get_right(),b.get_left()) if delta[0]>0 else (a.get_left(),b.get_right())
    else:
        start,end=(a.get_top(),b.get_bottom()) if delta[1]>0 else (a.get_bottom(),b.get_top())
    return Arrow(start,end,
                 buff=.09,color=color,stroke_width=2,tip_length=.14,
                 max_tip_length_to_length_ratio=.12)


def codeblock(lines,size=23,spacing=.36):
    objects=VGroup()
    styled=python_lines('\n'.join(lines))
    for i,(line,spans) in enumerate(zip(lines,styled)):
        count=len(line)-len(line.lstrip())
        obj=MarkupText(markup_line(spans,trim_start=count),font='Menlo',font_size=size,disable_ligatures=True)
        obj.move_to([count*.105,-i*spacing,0],aligned_edge=LEFT)
        objects.add(obj)
    return objects


class DetailScenes:
    def closure_detail(self):
        self.section('放慢一跳：纯计算如何变得可提取')
        self.heading('让依赖像多米诺骨牌一样，依次就绪')
        load=node('Load b[1]',[-4.7,1.65,0],PURPLE,26)
        get=node('b1 : c72',[-1.6,1.65,0],GREY_B,25)
        known=node('a1 : c71',[-1.6,-.05,0],BLUE,25)
        add=node('a1 + b1',[2.25,.8,0],GREY_B,28)
        edges=VGroup(link(load,get),link(get,add),link(known,add))
        labels=VGroup(text('c21',21,PURPLE).next_to(load,UP),text('c37',21,GREEN).next_to(add,UP))
        info=text('#73 → #200：选择一次真实读取',24,GREY_B).move_to([0,2.35,0])
        self.beat('closure_enter',lambda:self.play(FadeIn(load),FadeIn(get),FadeIn(known),FadeIn(add),Create(edges),FadeIn(labels),FadeIn(info),run_time=1.6))
        def domino():
            self.play(Flash(load,color=PURPLE),run_time=.5)
            self.play(get.animate.set_color(GREEN),edges[0].animate.set_color(GREEN),run_time=.8)
            self.play(Circumscribe(known,color=BLUE),Circumscribe(get,color=GREEN),run_time=.7)
            self.play(add.animate.set_color(GREEN),edges[1:].animate.set_color(GREEN),run_time=.8)
        self.beat('closure_and',domino)
        alternatives=text('c72 有 4 种候选；当前 Get(c21,0) 已经足够',23,BLUE).move_to([0,-1.05,0])
        formula=MathTex(r'\mathrm{available}(c)=\bigvee_{n\in c}\ \bigwedge_{u\in\mathrm{inputs}(n)}\mathrm{available}(u)',font_size=31).move_to([0,-1.75,0])
        self.beat('closure_or',lambda:self.play(FadeIn(alternatives),Write(formula),run_time=1.8))
        verified=self.flow['verified']['closures']
        note=text(f'这一跳 +11 项可提取信息 · Python 已核对 {verified:,} 次传播',23,GREY_B).move_to([0,-2.55,0])
        self.beat('closure_existing',lambda:self.play(FadeIn(note),Indicate(add,color=GREEN),run_time=1.2))
        self.shot('14-closure-detail')

    def relaxation_detail(self):
        self.section('每一次循环：合法、估价、去重、入队')
        self.heading('队列里放的是“带着信息的状态”')
        lines=['q = pop_cheapest()',
               'for n in effect_successors(q.position):',
               '    if missing_input(n, q.known): continue',
               '    cost = q.cost + estimate(n)',
               '    known = pure_closure(q.known | {n.eclass})',
               '    key = (n.eclass, live_known(n.eclass, known))',
               '    if cost < best[key]:',
               '        remember(key, cost, parent=q)',
               '        push(key)']
        code=codeblock(lines,22,.44).move_to([-1.5,.35,0])
        # Keep the left edge fixed; the right column holds the concrete example.
        code.to_edge(LEFT,buff=.65)
        focus=SurroundingRectangle(code[0],color=GOLD,buff=.08)
        self.beat('loop_step',lambda:self.play(FadeIn(code),Create(focus),run_time=1.6))
        score=MathTex('1225','+','611','=','1836',font_size=35,color=GOLD).move_to([4,1.6,0])
        origin=text('#73 → #200',22,GREY_B).next_to(score,UP,buff=.32)
        def cost_step():
            for i in [1,2,3]:self.play(Transform(focus,SurroundingRectangle(code[i],color=GOLD,buff=.08)),run_time=.65)
            self.play(Write(score),FadeIn(origin),run_time=1.2)
        self.beat('loop_cost',cost_step)
        key=VGroup(text('相同位置',25,PURPLE),text('+',25),text('相同相关信息',25,GREEN)).arrange(DOWN,buff=.2).move_to([4,-.5,0])
        def relax():
            for i in [4,5,6,7,8]:self.play(Transform(focus,SurroundingRectangle(code[i],color=GOLD,buff=.08)),run_time=.55)
            self.play(FadeIn(key),run_time=.8)
        self.beat('loop_relax',relax)
        disclaimer=text('教学伪代码：省略原生集合实现、活跃性计算与额外剪枝',22,GREY_B).move_to([0,-2.65,0])
        self.beat('loop_boundary',lambda:self.play(FadeIn(disclaimer),run_time=.8))
        self.shot('15-loop')

    def unroll_detail(self):
        self.section('路径 → 程序：把状态的重复出现展开')
        self.heading('同一个 c9，不等于同一次发生')
        positions=[[-5.3+i*2.12,.95,0] for i in range(6)]
        occurrences=VGroup(*[node('c9',p,PURPLE,27) for p in positions])
        loads=self.ir['loads']
        names=VGroup(*[text('Load '+r['label'],21,BLUE).next_to(occurrences[i],DOWN,buff=.35) for i,r in enumerate(loads)])
        path=VGroup(*[link(occurrences[i],occurrences[i+1],PURPLE) for i in range(5)])
        note=text('每次读取后的 Get(load,1)，在搜索图中都属于 c9',24,GREY_B).move_to([0,2.1,0])
        self.beat('unroll_problem',lambda:self.play(FadeIn(occurrences),FadeIn(names),Create(path),FadeIn(note),run_time=1.7))
        def expand():
            self.play(*[Transform(occurrences[i][1],Text(f's{i+1}',font='Menlo',font_size=27,color=PURPLE).move_to(occurrences[i][1])) for i in range(6)],run_time=1.5)
            new=text('输出程序：六个发生位置，保留前后依赖',25,GREEN).move_to(note)
            self.play(Transform(note,new),run_time=.8)
        self.beat('unroll_occurrence',expand)
        code=codeblock(['b0, s1 = m.load(b, 0, s0)',
                        'a1, s2 = m.load(a, 1, s1)',
                        'b1, s3 = m.load(b, 1, s2)'],28,.48).move_to([0,-1.55,0])
        self.beat('unroll_python',lambda:self.play(Write(code),run_time=2))
        self.shot('16-state-occurrences')

    def fill_dag_detail(self):
        self.section('补齐纯依赖，得到共享的数据流 DAG')
        self.heading('主线定下来，依赖仍需要补齐')
        load=node('6 Loads',[-4.8,1.25,0],PURPLE,26)
        alloc=node('Alloc',[-1.85,1.25,0],PURPLE,26)
        write=node('3 Writes',[1.6,1.25,0],PURPLE,26)
        ret=node('Return',[5,1.25,0],PURPLE,26)
        spine=VGroup(load,alloc,write,ret)
        edges=VGroup(link(load,alloc,PURPLE),link(alloc,write,PURPLE),link(write,ret,PURPLE))
        note=text('这里只概括副作用主线；每次写入仍要拿到地址和数值',23,GREY_B).move_to([0,2.2,0])
        self.beat('fill_skeleton',lambda:self.play(FadeIn(spine),Create(edges),FadeIn(note),run_time=1.5))
        sums=VGroup(*[node(f'a{i} + b{i}',[-4.7+3*i,-.45,0],GREEN,24) for i in range(3)])
        deps=VGroup(*[link(load,s,GREEN) for s in sums],*[link(s,write,GREEN) for s in sums])
        def fill():
            self.play(LaggedStart(*[FadeIn(s) for s in sums],lag_ratio=.3),Create(deps),run_time=2.0)
            self.play(Circumscribe(sums[0],color=GOLD),Circumscribe(write,color=GOLD),run_time=1.3)
        self.beat('fill_topology',fill)
        self.play(FadeOut(deps),FadeOut(sums),run_time=.5)
        pointer=node('out',[-1.85,-.6,0],BLUE,28)
        uses=VGroup(*[text(x,23,BLUE).move_to([xpos,-2.1,0]) for x,xpos in [('out[0]',-4.9),('out[1]',-1.8),('out[2]',1.3),('return out',4.7)]])
        fan=VGroup(link(alloc,pointer,BLUE),*[link(pointer,u,BLUE) for u in uses])
        self.beat('fill_pointer',lambda:self.play(FadeIn(pointer),FadeIn(uses),Create(fan),run_time=1.8))
        self.shot('17-shared-dataflow')

    def python_detail(self):
        self.section('运行自动生成的 Python：从输入到结果')
        self.heading('把选中的 IR，翻成可以运行的 Python')
        lines=[s.split('  #')[0] for s in self.flow['plain_python'].splitlines()]
        code=codeblock(lines,22,.32)
        code.move_to([-3.6,.1,0]);code.to_edge(LEFT,buff=.7)
        input_a=text('a = [2, 4, 6]',25,BLUE).move_to([3.65,2.15,0])
        input_b=text('b = [10, 20, 30]',25,BLUE).move_to([3.65,1.65,0])
        title=text('实际运行时的变量',24,GREY_B).move_to([3.65,1,0])
        stored=VGroup(*[text(f"{r['value_name']} = ?",23,GREY_B).move_to([2.3+(i%2)*2.65,.4-(i//2)*.55,0])
                        for i,r in enumerate(self.flow['rows'][:6])])
        result=text('out = 未分配',25,GREY_B).move_to([3.65,-1.55,0])
        self.beat('python_intro',lambda:self.play(FadeIn(code),FadeIn(input_a),FadeIn(input_b),FadeIn(title),FadeIn(stored),FadeIn(result),run_time=2.1))
        focus=SurroundingRectangle(code[1],color=GOLD,buff=.07)
        steps=self.flow['steps']
        def reads():
            self.add(focus)
            for i,step in enumerate(steps[:6]):
                value=step['environment'][step['value_name']]
                label=text(f"{step['value_name']} = {value:g}",23,BLUE).move_to(stored[i])
                self.play(Transform(focus,SurroundingRectangle(code[i+1],color=GOLD,buff=.07)),Transform(stored[i],label),run_time=.8)
        self.beat('python_reads',reads)
        def writes():
            for i,step in enumerate(steps[6:-1],6):
                self.play(Transform(focus,SurroundingRectangle(code[i+1],color=GOLD,buff=.07)),run_time=.45)
                if 'out' in step['environment']:
                    values=step['environment']['out']
                    rendered=', '.join('·' if v is None else f'{v:g}' for v in values)
                    self.play(Transform(result,text(f'out = [{rendered}]',25,GREEN).move_to(result)),run_time=.45)
        self.beat('python_writes',writes)
        done=text('131 组输入：IR 与 Python 一致',23,GREEN).move_to([3.65,-2.35,0])
        self.beat('python_done',lambda:self.play(Transform(focus,SurroundingRectangle(code[-1],color=GOLD,buff=.07)),FadeIn(done),Circumscribe(result,color=GREEN),run_time=1.6))
        self.shot('18-generated-python')
