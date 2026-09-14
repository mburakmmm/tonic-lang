class A:
    def add(self,x):
        return x+1
class B(A):
    def add(self,x):
        return super().add(x)+1
b=B()
i=0
s=0
while i<10000:
    s+=b.add(i)
    i+=1
print(s)
