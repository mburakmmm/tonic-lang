"""Semantic regression cases for lexical scope, call binding, and containers."""
CASES = [
'''def counter(n):
    def inc(x=1):
        nonlocal n
        n += x
        return n
    def read():
        return n
    return inc,read
inc,read=counter(10)
print(inc(),inc(x=3),read())
''',
'''def outer(x):
    def mid():
        def inner():
            return x
        return inner
    x=7
    return mid
print(outer(1)()())
''',
'''x=1
def outer():
    x=2
    def mid():
        global x
        x=3
        def inner():
            return x
        return inner
    return mid
print(outer()()(),x)
''',
'''def make():
    fs=[]
    for i in range(3):
        def f():
            return i
        fs += (f,)
    return fs
f=make()
print(f[0](),f[1](),f[2]())
''',
'''def factory(x):
    def f(a=x):
        return a,x
    x=9
    return f
f=factory(2)
print(f(),f(5))
''',
'''def stamp(n):
    print(n)
    return []
def f(a=stamp(1),*,b=stamp(2)):
    a += (1,)
    b += (2,)
    return a,b
print(f())
print(f())
''',
'''def f(a,/,b=2,*args,c=3,**kw):
    print(a,b,args,c,kw)
f(1,4,5,6,c=7,x=8,a=9)
f(*[1],**{'c':5,'z':6})
f(1,*[2,3],*[4],**{'c':5},y=6)
''',
'''def f(*args,**kw):
    return args,kw
print(f(*[1],x=f(*[2],y=3),**{'z':4}))
''',
'''def f(*args,**kw):
    print(args,kw)
a=[1]
def mutate():
    a[0]=2
    return 3
f(*a,k=mutate())
a[0]=1
f(0,*a,k=mutate())
''',
'''def mark(n):
    print(n)
    return n
def f(*args,**kw):
    print(args,kw)
f(k=mark(2),*[mark(1)])
''',
'''d={True:1,1.0:2,'x':3}
d[1]=4
d['y']=5
print(d,len(d),d[True])
print({'x':1,'y':2}=={'y':2,'x':1})
print({**d,'z':6,**{'x':7}})
''',
'''d={}
d[(1,'x')]=2
print(d[(1.0,'x')])
d['self']=d
print(d)
''',
'''d={'a':1,'b':2}
for k in d:
    d[k]=3
    print(k,d[k])
''',
'''print(1,2,sep=':',end='!')
print(3,4,sep=None,end=None)
''',
'''f=lambda a,b=2,/,*args,c=3,**kw:(a,b,args,c,kw)
print(f(1,4,5,c=6,x=7))
''',
'''def make(x):
    return lambda y=2:x+y
g=make(8)
print(g())
funcs=[]
for i in range(3):
    funcs+=(lambda:i,)
print(funcs[0](),funcs[2]())
''',
'''x=4
fact=lambda n:1 if n<2 else n*fact(n-1)
class C:
    x=10
    read=staticmethod(lambda: x)
    add=classmethod(lambda cls,n:cls.x+n)
print(fact(6),C.read(),C.add(3))
''',
'''def inner():
    yield 1
    yield 2
    return 9
def outer():
    first=(yield 'ready')
    print(first)
    result=yield from inner()
    print(result)
    yield from [3,4]
g=outer()
print(next(g),g.send('sent'),list(g))
''',
'''def guarded():
    try:
        yield 'start'
    except ValueError as error:
        yield 'caught '+str(error)
    yield 'end'
g=guarded()
print(next(g),g.throw(ValueError('boom')),next(g),g.close())
''',
'''def inner():
    received=yield 'ready'
    yield received
    return 9
def outer():
    result=yield from inner()
    print('delegate-result',result)
g=outer()
print(next(g),g.send('sent'))
try:
    next(g)
except StopIteration as error:
    print(error.value,error.args)
e=StopIteration(1,2)
print(e.value,e.args)
''',
'''def guarded():
    try:
        try:
            yield 'start'
        except ValueError as error:
            yield 'caught '+str(error)
    finally:
        print('inner-finally')
def outer():
    try:
        yield from guarded()
    finally:
        print('outer-finally')
g=outer()
print(next(g),g.throw(ValueError('boom')))
print(g.close())
''',
'''def catcher():
    try:
        yield 'ready'
    except Exception as error:
        yield type(error).__name__,error.args
def run(*arguments):
    g=catcher()
    print(next(g),g.throw(*arguments))
run(ValueError)
run(ValueError,'message')
run(ValueError,(1,2),None)
''',
'''async def inner(value):
    return value+1
async def outer():
    value=await inner(41)
    return [value]
coroutine=outer()
wrapper=coroutine.__await__()
print(type(coroutine).__name__,type(wrapper).__name__,iter(wrapper)==wrapper)
try:
    iter(coroutine)
except TypeError:
    print('not-iterable')
try:
    wrapper.send(None)
except StopIteration as error:
    print(error.value,error.args)
''',
'''class Pause:
    def __await__(self):
        received=yield 'paused'
        print('received',received)
        return 40
async def run():
    return (await Pause())+2
coroutine=run()
print(coroutine.send(None))
try:
    coroutine.send('resume')
except StopIteration as error:
    print(error.value)
''',
'''class Counter:
    def __init__(self,limit):
        self.i=0
        self.limit=limit
    def __aiter__(self):
        return self
    async def __anext__(self):
        if self.i>=self.limit:
            raise StopAsyncIteration
        value=self.i
        self.i+=1
        return value
async def collect():
    total=0
    async for value in Counter(4):
        if value==1:
            continue
        total+=value
    else:
        print('exhausted')
    return total
coroutine=collect()
try:
    coroutine.send(None)
except StopIteration as error:
    print(error.value)
''',
'''class Manager:
    def __init__(self,name,suppress=False):
        self.name=name
        self.suppress=suppress
    async def __aenter__(self):
        print('enter',self.name)
        return self.name+'-value'
    async def __aexit__(self,kind,value,traceback):
        print('exit',self.name,kind.__name__ if kind else 'None')
        return self.suppress
async def run():
    async with Manager('outer') as outer, Manager('inner') as inner:
        print(outer,inner)
    async with Manager('suppress',True):
        raise ValueError('hidden')
    return 'done'
coroutine=run()
try:
    coroutine.send(None)
except StopIteration as error:
    print(error.value)
''',
'''async def values():
    received=yield 1
    print('received',received)
    yield 2
async def collect():
    result=[]
    async for value in values():
        result=result+[value]
    return result
generator=values()
print(type(generator).__name__,generator.__aiter__()==generator)
first=generator.__anext__()
print(type(first).__name__,iter(first)==first)
try:
    first.send(None)
except StopIteration as error:
    print(error.value)
try:
    generator.asend('sent').send(None)
except StopIteration as error:
    print(error.value)
try:
    generator.aclose().send(None)
except StopIteration as error:
    print('closed',error.value)
coroutine=collect()
try:
    coroutine.send(None)
except StopIteration as error:
    print(error.value)
''',
'''import asyncio
async def worker(label):
    print('start',label)
    await asyncio.sleep(0)
    print('end',label)
    return label
async def main():
    first=asyncio.create_task(worker('a'))
    second=asyncio.create_task(worker('b'))
    print(await first,await second)
print(asyncio.run(main()))
''',
'''shared=[1]
alias=shared
print(shared is alias,shared is not [1],None is None)
print(2 in [1,2,3],4 not in (1,2,3),'bc' in 'abcd','x' in {'x':1},2 in range(4))
class Truth:
    def __init__(self,value): self.value=value
    def __bool__(self): return self.value
class Container:
    def __contains__(self,item): return Truth(item==7)
print(7 in Container(),8 not in Container())
class Needle:
    def __eq__(self,item): return item==2
def values():
    yield 1
    yield 2
    yield 3
print(Needle() in values())
class Meta(type):
    def __contains__(cls,item): return item==cls.answer
class TypeContainer(metaclass=Meta):
    answer=9
print(9 in TypeContainer,8 not in TypeContainer)
''',
'''x=99
print([x*y for x in range(5) if x%2 for y in range(3) if y])
print(x)
print({x:x*x for x in range(5) if x%2})
print([[x*y for y in range(3)] for x in range(4)])
print([a+b for a,b in [(1,2),(3,4)]])
def capture(offset):
    values=[offset+x for x in range(3)]
    mapping={x:offset+x for x in range(3)}
    stream=(offset+x for x in range(3))
    return values,mapping,stream
values,mapping,stream=capture(10)
print(values,mapping,list(stream))
funcs=[lambda: x for x in range(3)]
print(funcs[0](),funcs[1](),funcs[2]())
class Counter:
    def __init__(self): self.value=0
    def __iter__(self): return self
    def __next__(self):
        self.value+=1
        if self.value>3: raise StopIteration
        return self.value
print([value for value in Counter()])
class Key:
    def __init__(self,value): self.value=value
    def __hash__(self): return self.value%2
    def __eq__(self,other): return self.value==other.value
mapping={Key(value):[value] for value in range(3)}
print(len(mapping),mapping[Key(1)])
class Source:
    def __iter__(self):
        print('source-iter')
        return iter([1,2,3])
def element(value):
    print('element',value)
    return value*10
stream=(element(value) for value in Source())
print('made',type(stream).__name__)
print(next(stream),list(stream))
''',
'''import asyncio
class AsyncCompSource:
    def __init__(self,limit):
        self.value=0
        self.limit=limit
    def __aiter__(self):
        print('aiter',self.limit)
        return self
    async def __anext__(self):
        if self.value>=self.limit:
            raise StopAsyncIteration
        value=self.value
        self.value+=1
        return value
async def transform(value):
    return value*10
async def collect():
    values=[await transform(x) async for x in AsyncCompSource(4) if x%2]
    awaited=[await transform(x) for x in [2,3]]
    lambdas=[lambda value=await transform(x):value for x in [4,5]]
    mapping={x:await transform(x) async for x in AsyncCompSource(3)}
    nested=[x+y async for x in AsyncCompSource(2) for y in [10,20]]
    return values,awaited,lambdas[0](),lambdas[1](),mapping,nested
print(asyncio.run(collect()))
stream=(await transform(x) async for x in AsyncCompSource(3))
print(type(stream).__name__)
async def consume(stream):
    return [value async for value in stream]
print(asyncio.run(consume(stream)))
''',
'''import asyncio
values={1,3,1}
print(type(values).__name__,len(values),1 in values,2 not in values)
print(values=={3,1})
print({value%3 for value in range(8)}=={0,1,2})
print({value for value in []})
class SetSource:
    def __init__(self): self.value=0
    def __aiter__(self): return self
    async def __anext__(self):
        if self.value>=4: raise StopAsyncIteration
        value=self.value
        self.value+=1
        return value
async def collect_set():
    return {value%2 async for value in SetSource()}
print(asyncio.run(collect_set())=={0,1})
''',
'''def annotated(x: int, *args: str, y: float = 1, **kwargs: dict) -> str:
    return str(x)
def bare():
    pass
class Holder:
    def method(self, value: int) -> str:
        return str(value)
bare.__annotations__['late']=int
print(annotated.__annotations__)
print(bare.__annotations__)
print(Holder().method.__annotations__)
print(annotated(7))
''',
'''def subject():
    print('subject')
    return 2
def reject(value):
    print('guard',value)
    return False
match subject():
    case 1:
        print('one')
    case 2 as guarded if reject(guarded):
        print('guarded')
    case 2 | 3 as selected:
        print('selected',selected,guarded)
match 1:
    case True:
        print('bool')
    case 1:
        print('int')
class Codes:
    hit=7
match 7:
    case Codes.hit:
        print('qualified')
match [1,2,3,4]:
    case [first,*middle,last]:
        print(first,middle,last)
match (1,[2,3]):
    case [one,[two,three]]:
        print('nested',one,two,three)
match range(3):
    case [zero,*rest]:
        print('range',zero,rest)
match 'ab':
    case [left,right]:
        print('string-sequence')
    case other:
        print('string',other)
class Numbers(list):
    pass
match Numbers([5,6]):
    case [five,six]:
        print('subclass',five,six)
match {'a':[1,2,3],'b':4}:
    case {'a':[head,*tail],**remaining}:
        print('mapping',head,tail,remaining)
match {'other':1}:
    case {'missing':value}:
        print('unexpected')
    case fallback:
        print('missing',fallback)
class Mapping(dict):
    pass
match Mapping({'x':5}):
    case {'x':mapped}:
        print('dict-subclass',mapped)
class Key:
    def __init__(self,value):
        self.value=value
    def __hash__(self):
        return 7
    def __eq__(self,other):
        return isinstance(other,Key) and self.value==other.value
class Keys:
    target=Key('target')
match {Key('target'):9,'kept':10}:
    case {Keys.target:found,**rest}:
        print('custom-key',found,rest)
class Point:
    __match_args__=('x','y')
    def __init__(self,x,y):
        self.x=x
        self.y=y
class Colored(Point):
    pass
match Colored(3,4):
    case Point(x,4):
        print('class',x)
match 7:
    case int(value):
        print('builtin-class',value)
match Point(1,2):
    case Point(missing=value):
        print('unexpected-attribute')
    case other:
        print('missing-attribute',type(other).__name__)
class Probe:
    @property
    def value(self):
        print('get-value')
        return 8
match Probe():
    case Probe(value=8):
        print('descriptor')
def choose(value):
    match value:
        case None:
            return lambda:'none'
        case 4 as kept:
            def read():
                return kept
            return read
        case other:
            return lambda:other
print(choose(None)(),choose(4)(),choose(9)())
''',
'''def mark(value):
    print('mark',value)
    return value
name='Tönic'
width=6
print(f'hello {name} {mark(42):04d} {3.14159:.2f}')
print(f'{42:{width}d}',f'{name!r}',f'{name!a}',f'{name:*^9.3s}')
print(f'{1234567:,d}',f'{255:#06x}',f'{12345.678:.3g}',f'{-0.0:z.1f}',f'{name=}')
class Display:
    def __str__(self):
        print('str-call')
        return 'string'
    def __repr__(self):
        print('repr-call')
        return 'répr'
    def __format__(self,spec):
        print('format-call',spec)
        return '['+spec+']'
value=Display()
print(f'{value!s}',f'{value!r}',f'{value!a}',f'{value:custom}')
''',
'''class Annotated:
    value: int = 3
    missing: str
    if True:
        nested: list
print(Annotated.value,Annotated.__annotations__)
def local_annotation():
    hidden: missing_name
    return 7
print(local_annotation())
def owner():
    print('owner')
    return {}
def key():
    print('key')
    return 0
owner()[key()]: missing_name
box={}
box['item']: missing_name = 4
print(box)
''',
'''def identity[T: int](value: T) -> T:
    return value
def reveal[T]():
    def nested():
        return T
    return nested
class GenericBox[T]:
    seen=T
    item: T
    def reveal(self):
        return T
type PlainAlias = int
type PairAlias[T] = (T,T)
print(identity.__type_params__,identity.__annotations__,identity(7))
print(identity.__type_params__[0].__name__,identity.__type_params__[0].__bound__)
print(reveal()())
print(GenericBox.__type_params__,GenericBox.seen,GenericBox.__annotations__,GenericBox().reveal())
print(PlainAlias,PlainAlias.__name__,PlainAlias.__type_params__,PlainAlias.__value__)
print(PairAlias,PairAlias.__name__,PairAlias.__type_params__,PairAlias.__value__)
print(list[int],tuple[int,str],dict[str,int],list[int]([1,2]))
print(PairAlias[int],PairAlias[int].__origin__,PairAlias[int].__args__,PairAlias[int].__value__)
def shadow[T](T):
    return T
print(shadow(9),shadow.__type_params__)
''',
]
ERRORS = [
('result=[x async for x in source]', 'SyntaxError'),
('{{1}}', 'TypeError'),
('class C:\n    __match_args__=["x"]\nmatch C():\n    case C(value):\n        pass', 'TypeError'),
('class C:\n    __match_args__=(1,)\nmatch C():\n    case C(value):\n        pass', 'TypeError'),
('class C:\n    __match_args__=("x",)\nmatch C():\n    case C(first,second):\n        pass', 'TypeError'),
('class C:\n    __match_args__=("x",)\n    x=1\nmatch C():\n    case C(first,x=second):\n        pass', 'TypeError'),
('f"{1:.2d}"', 'ValueError'),
("f\"{'value':=8s}\"", 'ValueError'),
('class Bad:\n    def __format__(self,spec): return 1\nprint(f"{Bad()}")', 'TypeError'),
('class Keys:\n    first=1\n    second=True\nmatch {1:"x",2:"y"}:\n    case {Keys.first:left,Keys.second:right}:\n        pass', 'ValueError'),
('def f(**kw):\n    pass\nf(**{"x":1},x=print(2),y=print(3))', 'TypeError'),
('def f(**kw):\n    pass\nf(**{1:1},x=print(2))', 'TypeError'),
('def f(**kw):\n    pass\nf(**{1:1},**{True:2},x=print(2))', 'TypeError'),
('def f(**kw):\n    pass\nf(**{1:1},**{2:2},x=print(2))', 'TypeError'),
('def f(*a,**kw):\n    pass\nf(*None,x=print(2))', 'TypeError'),
('def f(*a,**kw):\n    pass\nf(0,*None,x=print(2))', 'TypeError'),
('{[]:print(1),2:print(2)}', 'TypeError'),
('{**{},[]:print(1),2:print(2)}', 'TypeError'),
('def f(a):\n    pass\nf(1,a=2)', 'TypeError'),
('def f(a,/):\n    pass\nf(a=1)', 'TypeError'),
('def f(*,a):\n    pass\nf()', 'TypeError'),
('def f(**kw):\n    pass\nf(x=1,**{"x":2})', 'TypeError'),
('def f(**kw):\n    pass\nf(**{1:2})', 'TypeError'),
('nonlocal x', 'SyntaxError'),
('def f():\n    nonlocal x', 'SyntaxError'),
('def f(x):\n    global x', 'SyntaxError'),
('def f():\n    print(x)\n    global x', 'SyntaxError'),
('def f():\n    def g():\n        return x\n    g()\n    x=1\nf()', 'NameError'),
("{}['x']", 'KeyError'),
('{[]:1}', 'TypeError'),
("d={'a':1}\nfor k in d:\n    d['b']=2", 'RuntimeError'),
('(lambda a:a)()', 'TypeError'),
('def bad():\n    yield 1\n    raise StopIteration("boom")\ng=bad()\nnext(g)\nnext(g)', 'RuntimeError'),
('def outer():\n    yield from [1,2]\ng=outer()\nnext(g)\ng.send(3)', 'AttributeError'),
('async def invalid():\n    await 1\ninvalid().send(None)', 'TypeError'),
('class Invalid:\n    def __await__(self):\n        return []\nasync def run():\n    await Invalid()\nrun().send(None)', 'TypeError'),
('async def done():\n    return 1\ncoroutine=done()\ntry:\n    coroutine.send(None)\nexcept StopIteration:\n    pass\ncoroutine.send(None)', 'RuntimeError'),
('async def bad():\n    raise StopIteration("escaped")\nbad().send(None)', 'RuntimeError'),
('class Bad:\n    def __aiter__(self):\n        return 1\nasync def run():\n    async for value in Bad():\n        pass\nrun().send(None)', 'TypeError'),
('class Bad:\n    def __aiter__(self):\n        return self\n    def __anext__(self):\n        return 1\nasync def run():\n    async for value in Bad():\n        pass\nrun().send(None)', 'TypeError'),
('class Once:\n    def __init__(self):\n        self.done=False\n    def __aiter__(self):\n        return self\n    async def __anext__(self):\n        if self.done:\n            raise StopAsyncIteration\n        self.done=True\n        return 1\nasync def run():\n    async for value in Once():\n        raise StopAsyncIteration("body")\nrun().send(None)', 'StopAsyncIteration'),
('class Bad:\n    def __aenter__(self):\n        return 1\n    async def __aexit__(self,a,b,c):\n        pass\nasync def run():\n    async with Bad():\n        pass\nrun().send(None)', 'TypeError'),
('class Bad:\n    async def __aenter__(self):\n        pass\nasync def run():\n    async with Bad():\n        pass\nrun().send(None)', 'TypeError'),
('class Bad:\n    async def __aenter__(self):\n        pass\n    def __aexit__(self,a,b,c):\n        return 1\nasync def run():\n    async with Bad():\n        raise ValueError("body")\nrun().send(None)', 'TypeError'),
('async def bad():\n    yield 1\n    raise StopAsyncIteration("escaped")\ngenerator=bad()\ntry:\n    generator.__anext__().send(None)\nexcept StopIteration:\n    pass\ngenerator.__anext__().send(None)', 'RuntimeError'),
('async def bad():\n    yield 1\n    return 2', 'SyntaxError'),
('async def bad():\n    yield from []', 'SyntaxError'),
('def annotated_local():\n    value: int\n    return value\nannotated_local()', 'UnboundLocalError'),
('def generic_missing[T]():\n    print(T)\n    T=1\ngeneric_missing()', 'UnboundLocalError'),
('1 in 2', 'TypeError'),
('1 in "123"', 'TypeError'),
('[hidden for hidden in range(2)]\nprint(hidden)', 'NameError'),
('class C:\n    values=[1]\n    result=[values for item in range(1)]', 'NameError'),
]
