a = cellfun(@(x) x + 1, {1, 2, 3});
b = cellfun(@plus, {1, 2; 3, 4}, {10, 20; 30, 40});
c = cellfun(@(x) [x x+1], {1, 2}, 'UniformOutput', false);
d = cellfun(@(s) string(s), {'a', 'b'});
