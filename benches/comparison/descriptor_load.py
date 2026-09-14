class D:
    def __get__(self,obj,owner):
        return obj._x
class C:
    x=D()
c=C()
c._x=1
i=0
s=0
while i<10000:
    s+=c.x
    i+=1
print(s)
