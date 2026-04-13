acc = 0;
for i = 1:4
    acc = acc + i;
end
n = 3;
while n > 0
    acc = acc + 1;
    n = n - 1;
end
if acc > 10
    flag = 1;
else
    flag = 0;
end
