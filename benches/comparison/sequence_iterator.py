class Sequence:
    def __init__(self, stop):
        self.stop = stop

    def __getitem__(self, index):
        if index >= self.stop:
            raise IndexError
        return index


i = 0
total = 0
while i < 100:
    total += sum(Sequence(1000))
    i += 1
print(total)
