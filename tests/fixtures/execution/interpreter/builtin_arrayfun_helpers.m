a = arrayfun(@(x) x + 1, [1 2 3]);
b = arrayfun(@plus, [1 2; 3 4], [10 20; 30 40]);
s = struct('x', {1, 2});
c = arrayfun(@(item) item.x + 1, s);
d = arrayfun(@(x) [x x+1], [1 2], 'UniformOutput', false);
e = arrayfun(@(x) string(x), [1 0 2]);
