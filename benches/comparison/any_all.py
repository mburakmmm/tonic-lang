false_values=[False for i in range(1000)]
true_values=[True for i in range(1000)]
i=0
total=0
while i<100:
    total+=any(false_values)
    total+=all(true_values)
    i+=1
print(total)
