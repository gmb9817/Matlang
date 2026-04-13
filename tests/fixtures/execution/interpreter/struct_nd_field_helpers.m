s = cat(3, [struct("value", 1), struct("value", 2)], [struct("value", 3), struct("value", 4)]);
a = reshape([s.value], size(s));
b = size(a);
s.value = cat(3, [10, 20], [30, 40]);
c = reshape([s.value], size(s));
d = size(c);
