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
]
