class C:
    pass
c=C()
c.x=1
i=0
s=0
while i<10000:
    s+=c.x
    i+=1
print(s)
