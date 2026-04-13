cells = {1, 2, 3};
[a, b, c] = cells{:};
[d, e, f] = deal(cells{:});
row = [cells{:}];
copy = {cells{:}};

s = struct('name', {'x', 'y'}, 'value', {10, 20});
[n1, n2] = s.name;
[v1, v2] = deal(s.value);
values = [s.value];
names = {s.name};
