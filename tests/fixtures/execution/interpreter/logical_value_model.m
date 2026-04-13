a = [1 2 3] > 1;
b = islogical(a);
c = isnumeric(a);
d = contains('alpha', 'a');
e = islogical(d);
f = string(d);
g = compose("flag=%s", d);
h = any(a);
i = all(a);
