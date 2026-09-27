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
]
ERRORS = [
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
('1 in 2', 'TypeError'),
('1 in "123"', 'TypeError'),
]
