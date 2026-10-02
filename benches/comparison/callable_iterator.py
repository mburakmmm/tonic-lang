class Counter:
    def __init__(self, stop):
        self.value = 0
        self.stop = stop

    def __call__(self):
        value = self.value
        self.value += 1
        return value


i = 0
total = 0
while i < 100:
    total += sum(iter(Counter(1000), 1000))
    i += 1
print(total)
