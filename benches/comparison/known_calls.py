def add(a,b):
    return a+b
i=0
s=0
while i<10000:
    s+=add(i,2)
    i+=1
print(s)
