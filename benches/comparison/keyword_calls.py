def add(a,/,b=2,*,bias=3):
    return a+b+bias
i=0
s=0
while i<10000:
    s+=add(i,b=2,bias=3)
    i+=1
print(s)
