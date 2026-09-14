def fib(n):
    a=0
    b=1
    while n>0:
        a,b=b,a+b
        n-=1
    return a
i=0
s=0
while i<1000:
    s+=fib(40)
    i+=1
print(s)
