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
'''class Descriptor:
    def __set_name__(self,owner,name):
        print('set_name',owner.__name__,name)
class Meta(type):
    @classmethod
    def __prepare__(mcls,name,bases):
        print('prepare',mcls.__name__,name,len(bases))
        return {'seed':4}
    def __new__(mcls,name,bases,namespace):
        print('new',mcls.__name__,name,len(bases),namespace['seed'])
        namespace['made']=namespace['seed']+1
        cls=super().__new__(mcls,name,bases,namespace)
        print('after_new',cls.__name__)
        return cls
    def __init__(cls,name,bases,namespace):
        print('init',cls.__name__,name,namespace['made'])
        cls.ready=namespace['made']+1
class C(metaclass=Meta):
    print('body',seed)
    item=Descriptor()
print(C.made,C.ready,type(C)==Meta)
class InitOnly(type):
    def __init__(cls,name,bases,namespace):
        cls.copied=namespace['value']
class I(metaclass=InitOnly):
    value=9
print(I.copied)
''',
'''class D:
    def __set_name__(self,owner,name):
        print('set_name',owner.__name__,name)
ns={'x':3,'d':D()}
C=type('C',(),ns)
print(C.__name__,C.__bases__[0]==object,C.x,C.__module__,C.__qualname__,C.__doc__)
print(len(ns),hasattr(ns,'__module__'))
class Base:
    value=5
Child=type('Child',(Base,),{'extra':7,'__module__':'custom'})
print(Child().value,Child.extra,Child.__module__)
''',
'''Mixed=type('Mixed',(),{1:2,'x':3})
view=Mixed.__dict__
seen=0
for key in view:
    if key==1:
        seen+=view[key]
print(view[1],view[1.0],seen,Mixed.x)
Mixed.y=4
del Mixed.x
print(view['y'],hasattr(Mixed,'x'))
''',
'''class Bag:
    def __init__(self):
        self.data={}
    def __getitem__(self,key):
        scratch=0.0
        for i in range(20):
            scratch+=0.5
        return self.data[key]+1
    def __setitem__(self,key,value):
        self.data[key]=value+2
        return 99
b=Bag()
b['x']=4
print(b['x'],b.data['x'])
b.__getitem__=lambda key:100
print(b['x'],b.__getitem__('x'))
class Static:
    __getitem__=staticmethod(lambda key:key+3)
    __setitem__=staticmethod(lambda key,value:print('static-set',key,value))
class ByClass:
    @classmethod
    def __getitem__(cls,key):
        return cls.__name__+key
print(Static()[4],ByClass()['!'])
Static()[1]=2
''',
'''values=[1,2,3]
del values[-2]
print(values)
data={'a':1,'b':2}
del data['a']
print(data)
class Bag:
    def __init__(self):
        self.data={'x':4,'y':5}
    def __delitem__(self,key):
        scratch=0.0
        for i in range(20):
            scratch+=0.5
        del self.data[key]
        print('deleted',key)
        return 99
b=Bag()
del b['x']
print(b.data)
b.__delitem__=lambda key:print('shadow',key)
del b['y']
print(b.data)
class Static:
    __delitem__=staticmethod(lambda key:print('static',key))
del Static()[7]
''',
'''print(type(1).__name__,type(True).__name__,type(None).__name__,type(1.5).__name__,type('x').__name__,type([]).__name__,type(()).__name__,type({}).__name__)
print(type(1)==int,isinstance(True,bool),isinstance(True,int),isinstance(1,object),issubclass(bool,int),isinstance(1,(str,int)))
print(type(range(3))==range,isinstance(range(3),range),issubclass(range,object))
print(int(),int(True),int(3.9),int('42'))
print(int('101',2),int('0xff',0),int('10',base=2))
print(float(),float(2),float('2.5'))
class Truth:
    def __bool__(self):
        total=0.0
        for i in range(20):
            total+=0.5
        return True
print(bool(),bool([]),bool([1]),bool(Truth()))
print(str(),str(12),list('ab'),tuple([1,2]),list(range(3)))
print(dict({'x':3}),dict(a=1),dict({'a':1},b=2),dict([('a',1),('b',2)]))
''',
'''class Missing:
    def __init__(self):
        self.present=7
    def __getattr__(self,name):
        scratch=0.0
        for i in range(20):
            scratch+=0.5
        return name+'!'
m=Missing()
m.__getattr__=lambda name:'shadow'
print(m.present,m.absent,getattr(m,'other'))
class StaticMissing:
    __getattr__=staticmethod(lambda name:'static-'+name)
class ClassMissing:
    @classmethod
    def __getattr__(cls,name):
        return cls.__name__+'-'+name
print(StaticMissing().x,ClassMissing().y)
class Meta(type):
    def __getattr__(cls,name):
        return cls.__name__+'-'+name
class C(metaclass=Meta):
    present=3
print(C.present,C.missing)
''',
'''print(type(ValueError('x')).__name__,isinstance(ValueError(),Exception),issubclass(TypeError,BaseException),str(RuntimeError('bad')))
''',
'''def fail(kind):
    scratch=0.0
    for i in range(20):
        scratch+=0.5
    if kind==0:
        int('bad')
    if kind==1:
        return 1//0
    raise KeyError('key')
try:
    fail(0)
except TypeError:
    print('wrong')
except (ValueError, LookupError) as error:
    print('caught',type(error).__name__,isinstance(error,Exception))
try:
    print(error)
except NameError:
    print('cleared')
try:
    try:
        fail(1)
    except ArithmeticError:
        raise
except ZeroDivisionError:
    print('reraised')
try:
    print('body')
except Exception:
    print('bad')
else:
    print('else')
try:
    fail(2)
except:
    print('bare')
''',
'''try:
    raise ValueError('outer')
except ValueError as outer:
    try:
        raise TypeError('inner')
    except TypeError as inner:
        pass
    try:
        print(inner)
    except NameError:
        print('inner-cleared')
    try:
        raise
    except ValueError:
        print('outer-restored')
try:
    print(outer)
except NameError:
    print('outer-cleared')
try:
    try:
        raise ValueError('first')
    except ValueError as failed:
        raise TypeError('second')
except TypeError:
    print('handler-error')
try:
    print(failed)
except NameError:
    print('failed-cleared')
for mode in range(2):
    try:
        raise LookupError('loop')
    except LookupError as loop_error:
        if mode==0:
            continue
        break
try:
    print(loop_error)
except NameError:
    print('loop-cleared')
def leave():
    try:
        raise ValueError('return')
    except ValueError as returned:
        return 7
print(leave())
try:
    raise
except RuntimeError:
    print('no-active')
''',
'''try:
    print('body')
finally:
    print('normal-final')
try:
    try:
        raise ValueError('boom')
    finally:
        print('exception-final')
except ValueError:
    print('exception-kept')
try:
    raise ValueError('handled')
except ValueError:
    print('handled')
else:
    print('bad-else')
finally:
    print('handler-final')
def leave(mode):
    try:
        if mode==0:
            return 10
        return 20
    finally:
        print('return-final',mode)
print(leave(0),leave(1))
def override():
    try:
        return 1
    finally:
        return 2
print('override',override())
def suppress():
    try:
        raise ValueError('suppressed')
    finally:
        return 3
print('suppress',suppress())
for i in range(3):
    try:
        if i==0:
            continue
        break
    finally:
        print('loop-final',i)
try:
    def fail_return():
        try:
            return 1
        finally:
            print('raising-final')
            raise TypeError('override')
    fail_return()
except TypeError:
    print('return-overridden')
try:
    try:
        raise ValueError('active')
    finally:
        try:
            raise
        except ValueError:
            print('active-in-final')
except ValueError:
    print('reraised-after-final')
try:
    try:
        raise ValueError('old')
    finally:
        raise TypeError('new')
except TypeError:
    print('exception-overridden')
''',
'''class Manager:
    def __init__(self,name,suppress=False):
        self.name=name
        self.suppress=suppress
    def __enter__(self):
        print('enter',self.name)
        return self.name+'-value'
    def __exit__(self,kind,value,traceback):
        print('exit',self.name,kind.__name__ if kind else 'None')
        return self.suppress
with Manager('normal') as value:
    print(value)
with Manager('outer') as outer, Manager('inner') as inner:
    print(outer,inner)
try:
    with Manager('propagate'):
        raise ValueError('boom')
except ValueError:
    print('propagated')
with Manager('suppress',True):
    raise LookupError('hidden')
print('suppressed')
def leave():
    with Manager('return'):
        return 7
print(leave())
for i in range(2):
    with Manager('loop'+str(i)):
        if i==0:
            continue
        break
class Truth:
    def __bool__(self):
        print('truth')
        return True
class TruthManager(Manager):
    def __exit__(self,kind,value,traceback):
        print('truth-exit',kind.__name__)
        return Truth()
with TruthManager('truth-manager'):
    raise TypeError('hidden')
def old_exit(self,kind,value,traceback):
    print('captured-old')
class Mutating:
    __exit__=old_exit
    def __enter__(self):
        Mutating.__exit__=lambda self,kind,value,traceback: print('new')
        return self
with Mutating():
    pass
class TargetManager(Manager):
    def __enter__(self):
        return [1]
try:
    with TargetManager('target') as (a,b):
        pass
except ValueError:
    print('target-error')
class Meta(type):
    def __enter__(cls):
        print('meta-enter')
        return cls.__name__
    def __exit__(cls,kind,value,traceback):
        print('meta-exit',kind.__name__ if kind else 'None')
class ManagedClass(metaclass=Meta):
    pass
with ManagedClass as class_name:
    print(class_name)
class Reraising(Manager):
    def __exit__(self,kind,value,traceback):
        print('bare-exit')
        raise
try:
    with Reraising('reraising'):
        raise KeyError('same')
except KeyError:
    print('bare-reraised')
class RaisingExit(Manager):
    def __exit__(self,kind,value,traceback):
        print('raising-exit',kind.__name__ if kind else 'None')
        raise TypeError('new')
try:
    with Manager('exit-outer'):
        with RaisingExit('exit-inner'):
            pass
except TypeError:
    print('exit-replaced')
''',
'''def divide(a,b):
    return a/b
i=0
while i<20:
    divide(20,2)
    i+=1
try:
    divide(1,0)
except ZeroDivisionError as error:
    print(type(error).__name__)
''',
'''class Counter:
    def __init__(self,n):
        self.i=0
        self.n=n
    def __iter__(self):
        return self
    def stop(self):
        raise StopIteration
    def __next__(self):
        scratch=0.0
        for i in range(20):
            scratch+=0.5
        if self.i>=self.n:
            self.stop()
        value=self.i
        self.i+=1
        return value
for value in Counter(4):
    print(value)
else:
    print('done')
class Recover:
    def __init__(self):
        self.first=True
    def __iter__(self):
        return self
    def __next__(self):
        if self.first:
            self.first=False
            try:
                raise StopIteration
            except StopIteration:
                return 9
        raise StopIteration
for value in Recover():
    print('recovered',value)
class Failing:
    def __iter__(self):
        return self
    def __next__(self):
        raise ValueError('iteration failed')
try:
    for value in Failing():
        pass
except ValueError as error:
    print(type(error).__name__)
''',
'''class Counter:
    def __init__(self,n):
        self.i=0
        self.n=n
    def __iter__(self):
        return self
    def stop(self):
        raise StopIteration
    def __next__(self):
        if self.i>=self.n:
            self.stop()
        value=str(self.i)
        self.i+=1
        return value
class Fresh:
    def __iter__(self):
        return Counter(2)
print(list(Counter(4)))
print(tuple(Counter(3)))
print(list(Fresh()))
a,b=Fresh()
print(a,b)
for count in [1,3]:
    try:
        a,b=Counter(count)
    except ValueError as error:
        print(str(error))
def collect(*values,marker):
    print(values,marker)
collect(*Counter(3),marker='single')
collect(*Counter(1),*Counter(2),marker='multiple')
class Failing:
    def __iter__(self):
        return self
    def __next__(self):
        raise ValueError('iteration failed')
for operation in [0,1]:
    try:
        if operation==0:
            list(Failing())
        else:
            collect(*Failing(),marker='failure')
    except ValueError as error:
        print(type(error).__name__)
''',
'''class Pair:
    def __init__(self,key,value,count):
        self.key=key
        self.value=value
        self.count=count
        self.i=0
    def __iter__(self):
        print('pair-iter',self.key)
        return self
    def __next__(self):
        if self.i>=self.count:
            raise StopIteration
        if self.i==0:
            item=self.key
        else:
            item=self.value
        self.i+=1
        print('pair-next',item)
        return item
class Outer:
    def __init__(self):
        self.i=0
    def __iter__(self):
        print('outer-iter')
        return self
    def __next__(self):
        if self.i>=2:
            raise StopIteration
        print('outer-next',self.i)
        if self.i==0:
            pair=Pair('a',1,2)
        else:
            pair=Pair('b',2,2)
        self.i+=1
        return pair
print(dict(Outer(),a=9,c=3))
print(dict([Pair('x',7,2)]))
class PairFactory:
    def __iter__(self):
        print('factory-iter')
        return Pair('z',8,2)
print(dict([PairFactory()]))
class TupleOuter:
    def __init__(self):
        self.done=False
    def __iter__(self):
        return self
    def __next__(self):
        if self.done:
            raise StopIteration
        self.done=True
        return ('d',4)
print(dict(TupleOuter()))
try:
    dict([Pair('short',0,1)])
except ValueError as error:
    print(str(error))
try:
    dict([('ok',1),Pair('short2',0,1)])
except ValueError as error:
    print(str(error))
class FailingPair:
    def __iter__(self):
        return self
    def __next__(self):
        raise ValueError('pair failed')
try:
    dict([FailingPair()])
except ValueError as error:
    print(str(error))
class StopAtIter:
    def __iter__(self):
        raise StopIteration
try:
    dict(StopAtIter())
except StopIteration:
    print('iter stop propagated')
''',
'''class PlainError(Exception):
    pass
print(PlainError(1,'two').args,str(PlainError(1,'two')))
class MyError(Exception):
    def __init__(self,message):
        self.label=message
try:
    try:
        raise KeyError('context')
    except KeyError:
        raise MyError('outer') from ValueError('cause')
except MyError as error:
    print(error.label,error.args,error.__traceback__==None)
    print(type(error.__cause__).__name__,type(error.__context__).__name__,error.__suppress_context__)
try:
    try:
        raise ValueError('implicit')
    except ValueError:
        raise TypeError('replacement')
except TypeError as error:
    print(error.__cause__,type(error.__context__).__name__,error.__suppress_context__)
try:
    try:
        raise LookupError('hidden')
    except LookupError:
        raise RuntimeError('clean') from None
except RuntimeError as error:
    print(error.__cause__,type(error.__context__).__name__,error.__suppress_context__)
class TraceManager:
    def __enter__(self):
        return self
    def __exit__(self,kind,value,traceback):
        print(type(value).__name__,traceback==None)
try:
    with TraceManager():
        raise ValueError('managed')
except ValueError:
    pass
''',
'''class Intercept:
    def __init__(self):
        object.__setattr__(self,'seen','')
        self.value=7
    def __getattribute__(self,name):
        if name!='seen':
            old=object.__getattribute__(self,'seen')
            object.__setattr__(self,'seen',old+'get:'+name+',')
        if name=='fallback':
            raise AttributeError('from hook')
        return object.__getattribute__(self,name)
    def __getattr__(self,name):
        return 'missing:'+name
    def __setattr__(self,name,value):
        old=object.__getattribute__(self,'seen')
        object.__setattr__(self,'seen',old+'set:'+name+',')
        object.__setattr__(self,name,value)
        return 99
    def __delattr__(self,name):
        old=object.__getattribute__(self,'seen')
        object.__setattr__(self,'seen',old+'del:'+name+',')
        object.__delattr__(self,name)
        return 99
x=Intercept()
print(x.value,x.fallback,getattr(x,'absent','default'),hasattr(x,'also_absent'))
x.value=9
print(setattr(x,'other',11),x.other)
print(delattr(x,'other'),hasattr(x,'other'))
print(x.seen)
try:
    x.__getattribute__('fallback')
except AttributeError:
    print('direct-error')
class FailingProperty:
    @property
    def item(self):
        raise AttributeError('property')
    def __getattr__(self,name):
        if name=='item':
            raise AttributeError('fallback')
        return 5
f=FailingProperty()
print(getattr(f,'item',42),hasattr(f,'item'),getattr(f,'other',42),hasattr(f,'other'))
''',
'''class Plain:
    pass
p=Plain()
p.__setattr__('x',3)
print(p.__getattribute__('x'))
p.__delattr__('x')
print(hasattr(p,'x'))
class Meta(type):
    def __getattribute__(cls,name):
        if name=='virtual':
            return 'virtual:'+type.__getattribute__(cls,'__name__')
        return type.__getattribute__(cls,name)
    def __setattr__(cls,name,value):
        type.__setattr__(cls,name,value+1)
        return 99
    def __delattr__(cls,name):
        print('meta-del',name)
        type.__delattr__(cls,name)
        return 99
class C(metaclass=Meta):
    base=2
print(C.base,C.virtual,getattr(C,'missing',8),hasattr(C,'missing'))
C.extra=4
print(C.extra,setattr(C,'other',6),C.other)
del C.extra
print(delattr(C,'other'),hasattr(C,'extra'),hasattr(C,'other'))
class Cached:
    def __init__(self):
        self.value=2
c=Cached()
total=0
for i in range(20):
    total+=c.value
def hook(self,name):
    if name=='value':
        return 7
    return object.__getattribute__(self,name)
Cached.__getattribute__=hook
print(total,c.value)
del Cached.__getattribute__
print(c.value)
''',
'''class Data:
    def __get__(self,obj,owner):
        return 'data:'+obj.__name__
    def __set__(self,obj,value):
        type.__setattr__(obj,'written',value)
    def __delete__(self,obj):
        type.__setattr__(obj,'deleted',True)
class NonData:
    def __get__(self,obj,owner):
        return 'nondata:'+obj.__name__
class Meta(type):
    data=Data()
    nondata=NonData()
    @property
    def prop(cls):
        return 'prop:'+cls.__name__
    @prop.setter
    def prop(cls,value):
        type.__setattr__(cls,'prop_value',value)
    @prop.deleter
    def prop(cls):
        type.__setattr__(cls,'prop_deleted',True)
class C(metaclass=Meta):
    data='class-data'
    nondata='class-nondata'
print(C.data,C.nondata,C.prop)
C.data=5
C.prop=6
print(C.written,C.prop_value,C.data)
del C.data
del C.prop
print(C.deleted,C.prop_deleted,C.data)
del C.nondata
print(C.nondata)
''',
'''items=[]
mapping={}
text='x'
print((1).__class__==int,items.__class__==list,mapping.__class__==dict,text.__class__==str,None.__class__.__name__)
print(object.__getattribute__(1,'__class__')==int)
''',
'''class I(int):
    def twice(self):
        return self+self
class F(float):
    pass
class S(str):
    pass
class L(list):
    def first(self):
        return self[0]
class T(tuple):
    pass
class D(dict):
    pass
class SpecialInt(int):
    def __add__(self,other):
        return 90+other
class Source:
    def __init__(self,values):
        self.values=values
        self.index=0
    def __iter__(self):
        return self
    def __next__(self):
        scratch=0.0
        for i in range(20):
            scratch+=0.5
        if self.index==len(self.values):
            raise StopIteration()
        value=self.values[self.index]
        self.index+=1
        return value
i=I('42')
f=F('2.5')
s=S('ab')
l=L(Source([1,2]))
t=T(Source([3,4]))
d=D(Source([('x',5)]))
i.tag='integer'; s.tag='string'; l.tag='list'; d.tag='dict'
print(type(i).__name__,i,i.twice(),i.tag,isinstance(i,int),type(i+1)==int)
print(type(f).__name__,f,f+0.5,isinstance(f,float),type(-f)==float)
print(type(s).__name__,s,s+'c',s.tag,len(s),isinstance(s,str),type(s+'c')==str)
print(type(l).__name__,l,l.first(),l.tag,len(l),isinstance(l,list),type(l[:])==list)
print(type(t).__name__,t,t[1],len(t),isinstance(t,tuple),type(t[:])==tuple)
print(type(d).__name__,d,d['x'],d.tag,len(d),isinstance(d,dict))
l += [7]
l[0]=9
d['y']=6
print(type(l).__name__,l,d)
print({i:'int',s:'str',t:'tuple'}[42],{i:'int',s:'str',t:'tuple'}['ab'],{i:'int',s:'str',t:'tuple'}[(3,4)])
print(list(l),tuple(t),dict(d))
print(SpecialInt(2)+1)
print(type(int(i)).__name__,type(float(f)).__name__,type(str(s)).__name__)
''',
'''class Number:
    def __init__(self,value):
        self.value=value
    def __add__(self,other):
        scratch=0.0
        for i in range(20):
            scratch+=0.5
        return Number(self.value+other.value)
    def __sub__(self,other): return self.value-other.value
    def __rsub__(self,other): return other.value-self.value
    def __mul__(self,other): return self.value*other.value
    def __truediv__(self,other): return self.value/other.value
    def __floordiv__(self,other): return self.value//other.value
    def __mod__(self,other): return self.value%other.value
    def __eq__(self,other): return self.value==other.value
    def __lt__(self,other): return self.value<other.value
    def __le__(self,other): return self.value<=other.value
    def __gt__(self,other): return self.value>other.value
    def __ge__(self,other): return self.value>=other.value
    def __neg__(self): return -self.value
    def __pos__(self): return self.value
    def __abs__(self): return 100+self.value
class Child(Number):
    def __radd__(self,other):
        return Number(other.value+self.value+1000)
a=Number(8); b=Number(3); c=Child(2)
print((a+b).value,(a+c).value,a-b,b-a,a*b,a/b,a//b,a%b)
print(a==Number(8),Number(8)!=Number(8),a!=b,a<b,a<=b,a>b,a>=b)
print(-a,+a,abs(a))
class Maybe:
    def __add__(self,other): return NotImplemented
class Reverse:
    def __radd__(self,other): return 17
print(Maybe()+Reverse())
class InPlace:
    def __iadd__(self,other): return NotImplemented
    def __add__(self,other): return 9
x=InPlace(); x+=1; print(x)
class Equal:
    def __eq__(self,other): return NotImplemented
x=Equal(); y=Equal()
print(x==x,x==y,x!=y,type(NotImplemented).__name__,str(NotImplemented))
class Truth:
    def __bool__(self): return True
class Weird:
    def __eq__(self,other): return Truth()
print(Weird()!=Weird())
class Meta(type):
    def __mul__(cls,other): return cls.__name__+other
class C(metaclass=Meta): pass
print(C*'!')
''',
]
ERRORS = [
    ('class C:\n    pass\nC(1)', 'TypeError'),
    ('class C:\n    def __init__(self):\n        return 3\nC()', 'TypeError'),
    ('class C:\n    def __init__(self,x):\n        pass\nC()', 'TypeError'),
    ('class C:\n    pass\nC().missing', 'AttributeError'),
    ('class B(bool):\n    pass', 'TypeError'),
    ('class R(range):\n    pass', 'TypeError'),
    ('class C(int,str):\n    pass', 'TypeError'),
    ("type('B',(bool,),{})", 'TypeError'),
    ('bool(NotImplemented)', 'TypeError'),
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
    ('class Meta(type):\n    def __init__(cls,name,bases,namespace):\n        return 1\nclass C(metaclass=Meta):\n    pass', 'TypeError'),
    ("type(1,(),{})", 'TypeError'),
    ("type('C',[],{})", 'TypeError'),
    ("type('C',(),[])", 'TypeError'),
    ("type('C',(1,),{})", 'TypeError'),
    ('class C:\n    __getitem__=1\nC()[0]', 'TypeError'),
    ('class C:\n    __setitem__=1\nC()[0]=2', 'TypeError'),
    ('class C:\n    pass\nC()[0]', 'TypeError'),
    ('class C:\n    pass\nC()[0]=2', 'TypeError'),
    ('x=[]\ndel x[0]', 'IndexError'),
    ("x={}\ndel x['missing']", 'KeyError'),
    ('del (1)[0]', 'TypeError'),
    ('class C:\n    __delitem__=1\ndel C()[0]', 'TypeError'),
    ("int('bad')", 'ValueError'),
    ("float('bad')", 'ValueError'),
    ('int(1.0,2)', 'TypeError'),
    ('list(1)', 'TypeError'),
    ('dict(1)', 'TypeError'),
    ("int('10',1)", 'ValueError'),
    ('int(10,2)', 'TypeError'),
    ('dict([(1,)])', 'ValueError'),
    ('class C:\n    __getattr__=1\nC().missing', 'TypeError'),
    ('class C:\n    __getattribute__=1\nC().missing', 'TypeError'),
    ('class C:\n    __setattr__=1\nC().x=1', 'TypeError'),
    ('class C:\n    __delattr__=1\ndel C().x', 'TypeError'),
    ("class C:\n    pass\nobject.__getattribute__(C(),1)", 'TypeError'),
    ("class C:\n    pass\nobject.__setattr__(C,'x',1)", 'TypeError'),
    ("class C:\n    pass\ntype.__getattribute__(C(),'x')", 'TypeError'),
    ("class C:\n    pass\ntype.__setattr__(C(),'x',1)", 'TypeError'),
    ("try:\n    int('bad')\nexcept 1:\n    pass", 'TypeError'),
    ('raise', 'RuntimeError'),
    ("raise TypeError('outer') from 1", 'TypeError'),
    ('class MissingEnter:\n    def __exit__(self,a,b,c):\n        pass\nwith MissingEnter():\n    pass', 'TypeError'),
    ('class MissingExit:\n    def __enter__(self):\n        pass\nwith MissingExit():\n    pass', 'TypeError'),
    ('class Bad:\n    def __iter__(self):\n        return 1\nfor value in Bad():\n    pass', 'TypeError'),
    ('class Bad:\n    def __iter__(self):\n        return self\nfor value in Bad():\n    pass', 'TypeError'),
]
