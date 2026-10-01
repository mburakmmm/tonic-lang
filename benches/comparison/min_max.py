values=list(range(1000))
i=0
total=0
while i<100:
    total+=min(values)
    total+=max(values)
    i+=1
print(total)
