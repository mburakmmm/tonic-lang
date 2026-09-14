class C:
    def add(self,x):
        return x+1
c=C()
i=0
s=0
while i<10000:
    s+=c.add(i)
    i+=1
print(s)
