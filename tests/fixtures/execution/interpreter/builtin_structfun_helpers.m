s = struct('a', 1, 'b', 2);
a = structfun(@(x) x + 1, s);
b = structfun(@(x) [x x+1], s, 'UniformOutput', false);
