def counter(n):
    def inc():
        nonlocal n
        n+=1
        return n
    return inc
f=counter(0)
i=0
while i<10000:
    value=f()
    i+=1
print(value)
