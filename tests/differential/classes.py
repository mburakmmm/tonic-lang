"""Class/shape stage oracle; no repr/address/GC timing comparisons."""
CASES = [
'''class Point:
    tag='point'
    def __init__(self,x,y=2):
        self.x=x
        self.y=y
    def total(self,/,*,scale=1):
        return (self.x+self.y)*scale
p=Point(3,y=4)
print(p.x,p.tag,p.total(scale=2))
f=p.total
p.x+=10
print(f(),Point.total(p),f.__self__==p,f.__func__==Point.total)
''',
'''x='global'
def factory():
    x='outer'
    class C:
        print(x)
        x='class'
        def read(self):
            return x,C
    return C
C=factory()
print(C.x,C().read()[0],C().read()[1]==C)
''',
'''x='global'
def factory():
    x='outer'
    class C:
        global x
        x='changed'
        def read(self):
            return x
    return C
print(factory()().read(),x)
''',
'''def factory():
    x=1
    class C:
        nonlocal x
        x+=1
        y=x
        def f(self):
            nonlocal x
            x+=1
            return x
    return C
C=factory()
print(C.y,C().f())
''',
'''class A:
    x=1
    def f(self):
        return 'A'
class B(A):
    pass
class C(A):
    def f(self):
        return 'C'
class D(B,C):
    pass
d=D()
print(d.f(),d.x)
A.x=2
print(d.x)
D.f=A.f
print(d.f())
print(D.__mro__[1]==B,D.__mro__[2]==C,D.__mro__[3]==A,D.__mro__[4]==object)
''',
'''class A:
    'A doc'
    __x=2
    def __init__(self):
        self.__y=3
    def f(self):
        return self.__x+self.__y
    class B:
        pass
print(A().f(),A._A__x,A.__doc__,A.B.__qualname__)
def factory():
    class Local:
        pass
    return Local
print(factory().__name__,factory().__qualname__)
''',
'''class A:
    def __private(self):
        class Inner:
            pass
        return Inner
print(A()._A__private().__qualname__)
''',
'''class Base:
    def __init__(self,a=2,/,*args,b=3,**kw):
        self.data=(a,args,b,kw)
    def read(self,*args,**kw):
        return self.data,args,kw
class Child(Base):
    pass
c=Child(*[4,5],**{'b':6,'a':7})
print(c.read(*[8],**{'x':9}))
print(Child().data)
''',
'''class C:
    def f(self):
        return 1
a=C()
b=C()
d={a:2,a.f:3}
print(a==a,a==b,a.f==a.f,a.f==b.f,d[a],d[a.f])
def free():
    return 7
a.f=free
print(a.f(),b.f())
''',
'''class A:
    x=2
class B(A):
    pass
b=B()
setattr(b,'y',3)
print(getattr(b,'x'),getattr(b,'y'),getattr(b,'missing',4),hasattr(b,'missing'))
print(isinstance(b,A),isinstance(b,B),isinstance(1,object),issubclass(B,A),issubclass(A,B))
print(isinstance(b,(B,1)),issubclass(B,(A,1)))
''',
'''class C:
    pass
c=C()
c.x=1
def owner():
    print('owner')
    return c
def rhs():
    print('rhs')
    return 2
owner().x+=rhs()
print(c.x)
''',
'''object=123
class C:
    pass
print(C.__mro__[1].__name__)
''',
'''class A:
    values=[]
a=A()
b=A()
a.values+=(1,)
print(b.values)
a.values=[2]
print(a.values,b.values)
''',
'''def make(v):
    class A:
        x=v
        class B:
            x=v
            def f(self):
                return v
    return A
print(make(7).B().f(),make(8).x)
''',
'''def mark(tag):
    print('eval',tag)
    def apply(value):
        print('apply',tag)
        return value
    return apply
def default():
    print('default')
    return 2
@mark('outer')
@mark('inner')
def f(x=default()):
    return x+1
print(f())
''',
'''def decorate(tag):
    print('eval',tag)
    def apply(cls):
        print('apply',tag)
        cls.tag=tag
        return cls
    return apply
@decorate('outer')
@decorate('inner')
class C:
    pass
print(C.tag)
''',
'''class A:
    x=4
    @staticmethod
    def add(a,b=1):
        return a+b
    @classmethod
    def read(cls,n):
        return cls.x+n
class B(A):
    x=10
print(A.add(2),A().add(3,4),A.read(3),B.read(5),B().read(6))
''',
'''def f(x):
    return x+1
sm=staticmethod(f)
cm=classmethod(f)
print(sm(4),sm.__func__==f,cm.__func__==f)
''',
'''class C:
    def __init__(self,x):
        self._x=x
    @property
    def x(self):
        return self._x
    @x.setter
    def x(self,value):
        self._x=value
        return 99
c=C(2)
print(c.x,C.x.fget==C.x.fget,C.x.fset==C.x.fset)
c.x=7
print(getattr(c,'x'))
setattr(c,'x',9)
print(c.x)
class D:
    pass
d=D()
d.x=100
def read(self):
    return 5
D.x=property(read)
print(d.x)
''',
'''class Data:
    def __init__(self):
        self.value=0
    def __get__(self,obj,owner):
        if obj==None:
            return self
        print('get',owner.__name__)
        return self.value
    def __set__(self,obj,value):
        print('set',value)
        self.value=value
        return 99
class Base:
    x=Data()
class Child(Base):
    pass
c=Child()
c.x=4
print(c.x,Child.x==Base.x)
setattr(c,'x',8)
print(getattr(c,'x'))
class NonData:
    def __get__(self,obj,owner):
        return 7
class C:
    y=NonData()
n=C()
print(n.y,C.y)
n.y=9
print(n.y,C.y)
''',
'''class D:
    def __init__(self,tag):
        self.tag=tag
    def __set_name__(self,owner,name):
        print('set_name',self.tag,owner.__name__,name)
        self.name=name
    def __get__(self,obj,owner):
        return getattr(self,'name','unset')
def decorate(cls):
    print('decorate',cls.__name__)
    return cls
@decorate
class Base:
    print('body')
    a=D('first')
    b=D('second')
class Child(Base):
    pass
print(Base.a,Base.b,Child.a)
late=D('late')
Base.c=late
print(Base.c)
''',
'''class D:
    def __get__(self,obj,owner):
        return 'managed'
    def __delete__(self,obj):
        print('descriptor delete')
        obj.deleted=True
        return 99
class C:
    x=D()
c=C()
print(c.x)
del c.x
print(c.deleted,c.x)
class P:
    def __init__(self):
        self._x=4
    @property
    def x(self):
        return self._x
    @x.deleter
    def x(self):
        print('property delete')
        del self._x
p=P()
print(p.x)
del p.x
print(getattr(p,'_x','gone'),P.x.fdel==P.x.fdel)
''',
'''class C:
    x=1
c=C()
c.y=2
del c.y
del C.x
print(getattr(c,'y','missing'),getattr(C,'x','missing'))
''',
'''class C:
    pass
a=C()
b=C()
a.x=1
b.x=2
def owner(tag,value):
    print('owner',tag)
    return value
del owner(1,a).x,owner(2,b).x
print(getattr(a,'x','gone'),getattr(b,'x','gone'))
def get(self):
    return 1
def delete(self):
    self.deleted=True
class P:
    x=property(get,None,delete)
p=P()
del p.x
print(p.deleted)
''',
'''class A:
    def f(self):
        return 'A'
    @classmethod
    def who(cls):
        return cls.__name__
    @property
    def value(self):
        return 4
class B(A):
    def f(self):
        return 'B'+super().f()
class C(A):
    def f(self):
        return 'C'+super().f()
class D(B,C):
    def f(self):
        return 'D'+getattr(super(),'f')()
    @classmethod
    def who(cls):
        return super().who()
    @property
    def value(self):
        return super().value+1
    def defining(self):
        def nested():
            return __class__
        return nested()
d=D()
print(d.f(),D.who(),d.value,d.defining()==D)
print(super(D,d).f(),super(D,D).who(),super(D,d).__self__==d,super(D,D).__self_class__==D)
class Descriptor:
    def __get__(self,obj,owner):
        return owner.__name__
class Parent:
    label=Descriptor()
class Child(Parent):
    def read(self):
        return super().label
print(Child().read())
class Lambda:
    owner=lambda self: __class__
print(Lambda().owner()==Lambda)
''',
'''class C:
    def __new__(cls,x):
        print('new',cls.__name__,x)
        self=super().__new__(cls)
        self.before=x+1
        return self
    def __init__(self,x):
        print('init',x)
        self.after=x+2
c=C(4)
print(c.before,c.after,c.__new__==C.__new__)
class Skip:
    def __new__(cls):
        return 42
    def __init__(self):
        print('must not run')
print(Skip())
class Base:
    def __new__(cls,x):
        value=object.__new__(cls)
        value.x=x
        return value
    def __init__(self,x):
        self.x+=1
class Child(Base):
    pass
print(Child(7).x)
class Payload:
    pass
class Rooted:
    def __new__(cls,p,*,tag):
        x=0.0
        for i in range(50):
            x+=0.5
        return object.__new__(cls)
    def __init__(self,p,*,tag):
        self.p=p
        self.tag=tag
p=Payload()
r=Rooted(p,tag='kept')
print(r.p==p,r.tag)
''',
'''class Callable:
    def __init__(self,bias):
        self.bias=bias
    def __call__(self,x=1,*,scale=1):
        return (self.bias+x)*scale
class Child(Callable):
    pass
c=Child(2)
print(c(),c(3,scale=2))
c.__call__=lambda x: 99
print(c(4),c.__call__(4))
class Sized:
    def __len__(self):
        total=0.0
        for i in range(40):
            total+=0.5
        return 5
s=Sized()
s.__len__=lambda: 8
print(len(s))
class BoolSized:
    def __len__(self):
        return True
print(len(BoolSized()))
class StaticCall:
    __call__=staticmethod(lambda x: x+1)
class ClassCall:
    @classmethod
    def __call__(cls):
        return cls.__name__
class StaticSized:
    __len__=staticmethod(lambda: 6)
class ClassSized:
    @classmethod
    def __len__(cls):
        return 7
print(StaticCall()(4),ClassCall()(),len(StaticSized()),len(ClassSized()))
''',
'''class Toggle:
    def __init__(self):
        self.calls=0
    def __bool__(self):
        self.calls+=1
        scratch=0.0
        for i in range(20):
            scratch+=0.5
        return self.calls<3
t=Toggle()
while t:
    print('tick')
print(t.calls,not t)
t.__bool__=lambda: True
print('yes' if t else 'no',t.calls)
print((t and 7)==t,(t or 7)==t,t.calls)
class Length:
    def __init__(self,n):
        self.n=n
    def __len__(self):
        return self.n
class Child(Length):
    pass
print(not Child(0),not Child(2),'yes' if Child(3) else 'no')
class Both:
    def __bool__(self):
        return False
    def __len__(self):
        return 4
print(not Both())
class StaticTruth:
    __bool__=staticmethod(lambda: True)
class ClassTruth:
    @classmethod
    def __bool__(cls):
        return cls.__name__=='ClassTruth'
print(not StaticTruth(),not ClassTruth())
class Dynamic:
    def __bool__(self):
        return True
    def __len__(self):
        return 0
d=Dynamic()
print(not d)
del Dynamic.__bool__
print(not d,not object())
''',
]
ERRORS = [
    ('class C:\n    pass\nC(1)', 'TypeError'),
    ('class C:\n    def __init__(self):\n        return 3\nC()', 'TypeError'),
    ('class C:\n    def __init__(self,x):\n        pass\nC()', 'TypeError'),
    ('class C:\n    pass\nC().missing', 'AttributeError'),
    ('class C(1):\n    print("body")', 'TypeError'),
    ('class A:\n    pass\nclass B(A,A):\n    print("body")', 'TypeError'),
    ('class A:\n    pass\nclass B:\n    pass\nclass X(A,B):\n    pass\nclass Y(B,A):\n    pass\nclass Z(X,Y):\n    print("body")', 'TypeError'),
    ('class C:\n    return 1', 'SyntaxError'),
    ('def f():\n    class C:\n        return 1', 'SyntaxError'),
    ('class C:\n    nonlocal x', 'SyntaxError'),
    ('x=object()\nx.a=1', 'AttributeError'),
    ('object.x=1', 'TypeError'),
    ('class C:\n    pass\ngetattr(C,1)', 'TypeError'),
    ('isinstance(1,2)', 'TypeError'),
    ('issubclass(1,object)', 'TypeError'),
    ('class C:\n    __qualname__=1', 'TypeError'),
    ('@1\ndef f():\n    pass', 'TypeError'),
    ('def f(cls):\n    pass\nclassmethod(f)()', 'TypeError'),
    ('class C:\n    x=classmethod(1)\nd={C.x:2}\nprint(d[C.x],C.x==C.x)\nC.x()', 'TypeError'),
    ('class C:\n    @property\n    def x(self):\n        return 1\nC().x=2', 'AttributeError'),
    ('class C:\n    x=property()\nC().x', 'AttributeError'),
    ('class D:\n    __get__=1\nclass C:\n    x=D()\nC().x', 'TypeError'),
    ('class D:\n    __set__=1\nclass C:\n    x=D()\nC().x=2', 'TypeError'),
    ('class D:\n    def __set_name__(self,owner,name):\n        return missing\nclass C:\n    x=D()', 'NameError'),
    ('class D:\n    __set_name__=1\nclass C:\n    x=D()', 'TypeError'),
    ('class C:\n    @property\n    def x(self):\n        return 1\ndel C().x', 'AttributeError'),
    ('class C:\n    pass\ndel C().missing', 'AttributeError'),
    ('class D:\n    def __delete__(self,obj):\n        pass\nclass C:\n    x=D()\nC().x=1', 'AttributeError'),
    ('super()', 'RuntimeError'),
    ('class C:\n    def f():\n        return super()\nC.f()', 'RuntimeError'),
    ('class A:\n    pass\nclass B:\n    pass\nsuper(A,B())', 'TypeError'),
    ('object.__new__(1)', 'TypeError'),
    ('class C:\n    __new__=1\nC()', 'TypeError'),
    ('class C:\n    pass\nobject.__new__(C,1)', 'TypeError'),
    ('class C:\n    pass\nC()()', 'TypeError'),
    ('class C:\n    __call__=1\nC()()', 'TypeError'),
    ('class C:\n    def __len__(self):\n        return -1\nlen(C())', 'ValueError'),
    ('class C:\n    def __len__(self):\n        return 1.0\nlen(C())', 'TypeError'),
    ('class C:\n    pass\nc=C()\nC.__call__=c\nc()', 'RecursionError'),
    ('class C:\n    def __bool__(self):\n        return 1\nif C():\n    pass', 'TypeError'),
    ('class C:\n    def __len__(self):\n        return -1\nif C():\n    pass', 'ValueError'),
    ('class C:\n    def __len__(self):\n        return 1.0\nnot C()', 'TypeError'),
    ('class C:\n    __bool__=1\nC() and 1', 'TypeError'),
]
