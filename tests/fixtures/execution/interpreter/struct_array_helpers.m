s = [struct() struct(); struct() struct()];
s.name = ["alpha" "beta"; "gamma" "delta"];
s.value = [1 2; 3 4];
s.inner.flag = [true false; false true];
a = isstruct(s);
b = isfield(s, 'name');
c = fieldnames(s);
d = [s.name];
e = [s.value];
f = [s.inner.flag];
g = rmfield(s, "value");
h = isfield(g, 'value');
i = [g.name];

[u(1:2).field1] = deal([10 20], [14 12]);
j = u;

[q(1:2).field1] = [31 32];
m = q;

v = struct('field1', {0, 0});
[v.field1] = deal(10, 20);
k = v;

w = struct('name', {'a', 'b'});
[w.inner.score] = deal(81, 83);
l = [w.inner.score];

[x.field1] = [31 32];
m2 = x;

[y.inner.score] = [41 42];
n2 = y;

plain.field1 = [51 52];
o = plain;

plain_nested.inner.field1 = [61 62];
o2 = plain_nested;

plain_deal.field1 = deal([71 72]);
o3 = plain_deal;
