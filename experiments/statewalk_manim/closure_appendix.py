"""Appended lessons: pure availability, cost choices, and scoped If interfaces."""
from manim import *
from film import StatewalkFilm, zh, math as formula, token, arrow, DATA
from details import node, link

BLUE='#58C4DD';GREEN='#83C167';PURPLE='#B189F7';GOLD='#FFFF00';RED='#FC6255';DIM='#646775'


def card(lines, at, color=BLUE, size=26, width=None):
    label=Text(lines,font='Menlo',font_size=size,line_spacing=.8,color=color).move_to(at)
    box=RoundedRectangle(width=width or max(1.4,label.width+.4),height=label.height+.4,
                         corner_radius=.12,color=color,fill_color=color,fill_opacity=.05).move_to(label)
    return VGroup(box,label)


class ClosureAppendix(StatewalkFilm):
    def construct(self):
        self.types()
        self.region_vs_closure()
        self.alternatives()
        self.availability_vs_cost()
        self.if_interface()
        self.if_rules()
        self.wrap_up()
        self.play(*[FadeOut(m) for m in list(self.mobjects) if m is not self.watermark],run_time=1)
        self.wait(.5)
        self.finish_edit()

    def types(self):
        self.section('补充一：state 元组与普通数值')
        title=self.heading('先分清：携带状态，与依赖状态')
        intro=VGroup(zh('state',42,PURPLE),zh('pure closure',42,GREEN),zh('bag',42,GOLD)).arrange(RIGHT,buff=1.2)
        note=zh('教学图形 · 对照 eggcc / Tiger 实现',24,GREY_B).move_to([0,-1.2,0])
        self.beat('extra_intro',lambda:self.play(FadeIn(intro),FadeIn(note),run_time=1.4))
        load=node('Load(ptr, s0)',[-4.7,.4,0],PURPLE,25)
        pair=card('(value, s1)',[-1,.4,0],PURPLE,31)
        value=node('Get(load,0)',[3,1.35,0],BLUE,25)
        state=node('Get(load,1)',[3,-.65,0],PURPLE,25)
        edges=VGroup(link(load,pair),link(pair,value,BLUE),link(pair,state,PURPLE))
        labels=VGroup(zh('普通数值',24,BLUE).next_to(value,RIGHT,buff=.3),zh('后继状态',24,PURPLE).next_to(state,RIGHT,buff=.3))
        def split():
            self.play(FadeOut(intro),FadeOut(note),FadeIn(load),FadeIn(pair),run_time=.7)
            self.play(Create(edges),FadeIn(value),FadeIn(state),FadeIn(labels),run_time=1.5)
            self.play(Circumscribe(pair,color=PURPLE),run_time=1)
        self.beat('extra_tuple',split)
        warning=zh('数值没有 state 类型，依然要等 Load 有合法来源',27,GOLD).move_to([0,-1.8,0])
        distinction=zh('Tiger 类型标记 ≠ 用于代码移动的 ExprIsPure',23,GREY_B).move_to([0,-2.5,0])
        self.beat('extra_purity',lambda:self.play(FadeIn(warning),FadeIn(distinction),Indicate(edges[0],color=GOLD),run_time=1.2))
        self.shot('20-effect-tuple')

    def region_vs_closure(self):
        self.section('补充二：纯区域，不等于 pure closure')
        self.heading('同一块纯计算，不会同时就绪')
        effect=[node('Load 1',[-5.6,1.2,0],PURPLE,27),node('Load 2',[-5.6,-1.1,0],PURPLE,27)]
        vals=[node('x',[-2.8,1.2,0],GREY_B),node('y',[-2.8,-1.1,0],GREY_B)]
        ops=[node('u=x+1',[.1,1.2,0],GREY_B,26),node('v=y+1',[.1,-1.1,0],GREY_B,26)]
        result=node('z=u+v',[3.8,.05,0],GREY_B,29)
        edges=VGroup(*[link(effect[i],vals[i]) for i in range(2)],*[link(vals[i],ops[i]) for i in range(2)],*[link(ops[i],result) for i in range(2)])
        state_link=Arrow(effect[0].get_bottom(),effect[1].get_top(),buff=.12,color=PURPLE,stroke_width=2)
        state_label=Text('state',font='Menlo',font_size=18,color=PURPLE).next_to(state_link,LEFT,buff=.1)
        area=DashedVMobject(RoundedRectangle(width=9.5,height=3.65,corner_radius=.18,color=BLUE),num_dashes=60).move_to([.8,.05,0])
        label=zh('相连的纯计算区域',23,BLUE).move_to([.8,2.2,0])
        foot=zh('常量 1 已知；省略状态投影；此图为教学示意',22,GREY_B).move_to([0,-2.45,0])
        self.beat('extra_region',lambda:self.play(Create(area),FadeIn(label),FadeIn(VGroup(*effect,*vals,*ops,result)),Create(edges),Create(state_link),FadeIn(state_label),FadeIn(foot),run_time=1.8))
        def first():
            self.play(Indicate(effect[0],color=PURPLE),run_time=.6)
            self.play(vals[0].animate.set_color(GREEN),edges[0].animate.set_color(GREEN),run_time=.7)
            self.play(ops[0].animate.set_color(GREEN),edges[2].animate.set_color(GREEN),run_time=.7)
            self.play(Indicate(result,color=RED),run_time=.7)
        self.beat('extra_first',first);self.shot('21-partial-pure-region')
        def second():
            self.play(Indicate(effect[1],color=PURPLE),run_time=.6)
            self.play(vals[1].animate.set_color(GREEN),edges[1].animate.set_color(GREEN),run_time=.7)
            self.play(ops[1].animate.set_color(GREEN),edges[3].animate.set_color(GREEN),run_time=.7)
            self.play(result.animate.set_color(GREEN),edges[4:].animate.set_color(GREEN),run_time=.8)
        self.beat('extra_second',second);self.shot('22-closure-grown')

    def alternatives(self):
        self.section('补充三：候选是“或”，还可能有环')
        title=self.heading('一个候选就够了，不必满足整片依赖')
        outer=RoundedRectangle(width=7.6,height=2.65,corner_radius=.2,color=BLUE).move_to([.65,.55,0])
        label=zh('e-class r',28,BLUE).move_to([.65,1.55,0])
        x=node('x',[-5.3,.4,0],DIM)
        sub=node('Sub(x,x)',[-1.4,.4,0],DIM,30)
        zero=node('Const(0)',[2.3,.4,0],GREEN,30)
        edge=link(x,sub,DIM)
        result=zh('r 可提取：选择 Const(0)',30,GREEN).move_to([.65,-1.65,0])
        note=zh('整数例子；值的依赖可绕过，effect 是否保留仍看状态链',22,GREY_B).move_to([0,-2.5,0])
        self.beat('extra_alternative',lambda:self.play(Create(outer),FadeIn(label),FadeIn(x),FadeIn(sub),FadeIn(zero),Create(edge),FadeIn(result),FadeIn(note),Circumscribe(zero,color=GREEN),run_time=2.1))
        self.shot('23-alternative-zero')
        eq=formula(r'x\equiv x+0',size=51,color=BLUE).move_to([0,1.5,0])
        cls=RoundedRectangle(width=5.9,height=1.5,corner_radius=.2,color=BLUE).move_to([0,-.1,0])
        body=Text('class c:  Arg | Add(c, 0)',font='Menlo',font_size=28,color=WHITE).move_to(cls)
        back=CurvedArrow([1.7,-.45,0],[-2.8,-.45,0],angle=-PI/2,color=GOLD)
        rule=zh('程序无循环 ≠ 候选 e-graph 无环',30,GOLD).move_to([0,-2.4,0])
        def cycle():
            self.play(*[FadeOut(m) for m in [outer,label,x,sub,zero,edge,result,note]],Transform(title,zh('有限程序，也可以来自有环的候选图',35).move_to(title)),run_time=.8)
            self.play(Write(eq),Create(cls),FadeIn(body),Create(back),FadeIn(rule),run_time=1.7)
        self.beat('extra_cycle',cycle);self.shot('24-candidate-cycle')

    def availability_vs_cost(self):
        self.section('补充四：搜索信息，与成本账本')
        title=self.heading('位置相同，能用的东西不同')
        a,b=[next(s for s in self.trace['state_graph'] if s['id']==i) for i in [11,73]]
        assert a['position']==b['position']==9 and a['values']==['b[0]'] and b['values']==['a[1]','b[0]']
        cards=VGroup(card('#11: c9\nb0',[-3.1,.6,0],PURPLE,35,width=4.2),card('#73: c9\nb0, a1',[3.1,.6,0],PURPLE,35,width=4.2))
        labels=VGroup(zh(f'可提取总数：{a["available_count"]}',25,GREEN).next_to(cards[0],DOWN,buff=.35),zh(f'可提取总数：{b["available_count"]}',25,GREEN).next_to(cards[1],DOWN,buff=.35))
        foot=zh('真实 vec_add 记录 · 卡片只摘录读取值',23,GREY_B).move_to([0,-2.25,0])
        self.beat('extra_two_records',lambda:self.play(FadeIn(cards),FadeIn(labels),FadeIn(foot),run_time=1.3));self.shot('25-state-versus-available')
        left=VGroup(zh('pure closure',35,GREEN),zh('哪些结果可以提取？',28,GREEN),formula(r'\{b_0,a_1,\ldots\}',size=36,color=GREEN)).arrange(DOWN,buff=.5).move_to([-3.2,.2,0])
        right=VGroup(zh('候选成本账本',35,GOLD),formula(r'\mathrm{sum}=4',size=34,color=GOLD),formula(r'\mathrm{bag}=\{t\mapsto3\}',size=34,color=GOLD)).arrange(DOWN,buff=.4).move_to([3.2,.2,0])
        sep=Line([0,1.7,0],[0,-1.6,0],color=GREY_D)
        note=zh('可提取 ≠ 已选用 ≠ 已付费',29,WHITE).move_to([0,-2.35,0])
        def jobs():
            self.play(FadeOut(cards),FadeOut(labels),FadeOut(foot),Transform(title,zh('两种信息，回答两种问题',35).move_to(title)),run_time=.8)
            self.play(FadeIn(left),FadeIn(right),Create(sep),FadeIn(note),run_time=1.3)
        self.beat('extra_two_jobs',jobs);self.shot('26-two-jobs')
        self.section('补充五：为什么 greedy 不保证整体最优')
        self.heading('便宜的单独方案，可能丢掉共享机会')
        first=card('A: independent\ncost = 4',[-3.5,1,0],BLUE,30,width=5.2)
        second=card('A: use t\ncost = 3 + 2 = 5',[3.5,1,0],GREY_B,30,width=5.2)
        chosen=zh('只看 A：选择左边',29,BLUE).move_to([0,-.6,0])
        note=zh('教学成本；两个方案属于同一 e-class A',23,GREY_B).move_to([0,-2.45,0])
        self.beat('extra_greedy',lambda:self.play(FadeIn(first),FadeIn(second),FadeIn(chosen),FadeIn(note),Circumscribe(first,color=BLUE),run_time=1.8))
        bcost=zh('B 也需要 t：3 + 1 = 4',28,PURPLE).move_to([0,-.4,0])
        lcost=formula(r'4+(3+1)=8',size=39,color=RED).move_to([-3.5,-1.45,0])
        rcost=formula(r'3+2+1=6',size=39,color=GREEN).move_to([3.5,-1.45,0])
        self.beat('extra_shared_choice',lambda:self.play(FadeOut(chosen),FadeIn(bcost),Write(lcost),Write(rcost),second.animate.set_color(GREEN),run_time=1.7));self.shot('27-greedy-counterexample')

    def if_interface(self):
        self.section('补充六：If 的分支作用域与统一出口')
        title=self.heading('If 接上两个分支，但只执行一个')
        syntax=Text('(If pred inputs then else)',font='Menlo',font_size=30,color=BLUE).move_to([0,2.45,0])
        ins=card('inputs\n(ptr, s0)',[-5.35,0,0],PURPLE,25,width=2)
        args=[node('Arg(ctx_T)',[-2.7,1.15,0],BLUE,23),node('Arg(ctx_F)',[-2.7,-1.15,0],BLUE,23)]
        bodies=[card('write 0\nreturn (0,sT)',[.65,1.15,0],PURPLE,24,width=3),card('write 1\nreturn (1,sF)',[.65,-1.15,0],PURPLE,24,width=3)]
        result=card('If result',[4.8,0,0],PURPLE,25,width=2.1)
        edges=VGroup(link(ins,args[0]),link(ins,args[1]),link(args[0],bodies[0]),link(args[1],bodies[1]),link(bodies[0],result),link(bodies[1],result))
        foot=zh('真实 IR 接口；Write / return 为教学简写',22,GREY_B).move_to([0,-2.7,0])
        self.beat('extra_if_interface',lambda:self.play(FadeIn(syntax),FadeIn(ins),FadeIn(VGroup(*args,*bodies,result)),Create(edges),FadeIn(foot),run_time=1.7))
        ctx=[Text('InIf(true, pred, inputs)',font='Menlo',font_size=18,color=BLUE).move_to([-.95,1.97,0]),Text('InIf(false, pred, inputs)',font='Menlo',font_size=18,color=BLUE).move_to([-.95,-1.97,0])]
        self.beat('extra_if_context',lambda:self.play(*[FadeIn(c) for c in ctx],Indicate(args[0],color=BLUE),Indicate(args[1],color=BLUE),run_time=1.4));self.shot('28-if-contexts')
        condition=zh('第 1 次执行：pred = true',27,GREEN).move_to([0,2.45,0])
        def select():
            self.play(FadeOut(syntax),FadeIn(condition),run_time=.5)
            dot=Dot(ins.get_center(),color=GOLD,radius=.075);self.add(dot)
            for obj in [args[0],bodies[0],result]:self.play(dot.animate.move_to(obj.get_center()),run_time=.65)
            self.play(FadeOut(dot),Transform(condition,zh('第 2 次执行：pred = false',27,GREEN).move_to(condition)),run_time=.6)
            dot=Dot(ins.get_center(),color=GOLD,radius=.075);self.add(dot)
            for obj in [args[1],bodies[1],result]:self.play(dot.animate.move_to(obj.get_center()),run_time=.65)
            self.play(FadeOut(dot),run_time=.3)
        self.beat('extra_if_select',select)
        equation=formula(r's_T=s_F',size=46,color=RED).move_to(condition)
        cross=Cross(equation,stroke_color=RED,stroke_width=5)
        output=Text('s_after = Get(If_result, 1)',font='Menlo',font_size=25,color=PURPLE).move_to([0,-2.55,0])
        exit_link=VGroup(Line(result.get_bottom(),[4.8,-2.55,0],color=PURPLE),Arrow([4.8,-2.55,0],output.get_right(),buff=.08,color=PURPLE))
        def no_union():
            self.play(FadeOut(condition),Write(equation),Create(cross),FadeOut(foot),FadeOut(ctx[1]),FadeIn(output),Create(exit_link),Circumscribe(result,color=PURPLE),run_time=1.5)
        self.beat('extra_if_not_union',no_union);self.shot('29-if-is-not-union')

    def if_rules(self):
        self.section('补充七：哪些信息可以穿过分支边界')
        title=self.heading('有规则跨越边界，但每条都有条件')
        columns=VGroup(
            VGroup(zh('ContextOf',33,BLUE),zh('跟踪作用域',27,WHITE)).arrange(DOWN,buff=.5),
            VGroup(zh('Subst',33,PURPLE),zh('把 inputs 接给 Arg',27,WHITE)).arrange(DOWN,buff=.5),
            VGroup(zh('区间分析',33,GREEN),formula(r'[2,4],\ [3,8]\ \to [2,8]',size=29,color=GREEN)).arrange(DOWN,buff=.5)
        ).arrange(RIGHT,buff=.8).move_to([0,.6,0])
        if columns.width>12.5:columns.scale_to_fit_width(12.5)
        info=zh('区间汇总的是数值范围，不是在判等两个分支',25,GREY_B).move_to([0,-1.6,0])
        self.beat('extra_context_rules',lambda:self.play(FadeIn(columns),FadeIn(info),run_time=1.4))
        start=node('s0',[-5,0,0],PURPLE,35)
        upper=card('then\nreturn (0,s0)',[-1.5,1.1,0],BLUE,26,width=3)
        lower=card('else\nreturn (1,s0)',[-1.5,-1.1,0],BLUE,26,width=3)
        end=node('s_after',[4.7,0,0],PURPLE,28)
        links=VGroup(link(start,upper),link(start,lower),link(upper,end),link(lower,end))
        value=card('If(value only)',[.6,0,0],BLUE,25,width=3.2)
        direct=VGroup(Line(start.get_bottom(),[-5,-1.45,0],color=PURPLE,stroke_width=3),
                      Line([-5,-1.45,0],[4.7,-1.45,0],color=PURPLE,stroke_width=3),
                      Arrow([4.7,-1.45,0],end.get_bottom(),buff=.05,color=PURPLE,stroke_width=3))
        guard=zh('两边都原样返回输入状态；数值 0 / 1 仍依赖条件',25,GOLD).move_to([0,-2.5,0])
        def passthrough():
            self.play(FadeOut(columns),FadeOut(info),Transform(title,zh('特殊情况：状态直通，其他结果仍可不同',35).move_to(title)),run_time=.7)
            self.play(FadeIn(VGroup(start,upper,lower,end)),Create(links),FadeIn(guard),run_time=1.2)
            self.play(Circumscribe(upper,color=PURPLE),Circumscribe(lower,color=PURPLE),run_time=1)
            self.play(FadeOut(upper),FadeOut(lower),FadeOut(links),FadeIn(value),Create(direct),run_time=1.4)
        self.beat('extra_passthrough',passthrough);self.shot('30-conditional-state-passthrough')

    def wrap_up(self):
        self.section('补充总结：四份信息，各有职责')
        self.heading('保持区分，才能正确共享')
        items=[('state','副作用的连接顺序',PURPLE),('context','参数作用域与条件假设',BLUE),('pure closure','传播哪些结果可提取',GREEN),('bag','候选方案的共享成本',GOLD)]
        cards=VGroup()
        for i,(name,meaning,color) in enumerate(items):
            content=VGroup(zh(name,35,color),zh(meaning,27,WHITE)).arrange(DOWN,buff=.4).move_to([(-3.3 if i%2==0 else 3.3),(1.1 if i<2 else -1.1),0])
            box=SurroundingRectangle(content,buff=.35,color=color,corner_radius=.15)
            cards.add(VGroup(box,content))
        self.beat('extra_summary',lambda:self.play(LaggedStart(*[FadeIn(c) for c in cards],lag_ratio=.2),run_time=2));self.shot('31-four-kinds-of-information')
