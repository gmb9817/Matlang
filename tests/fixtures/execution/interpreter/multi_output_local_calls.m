[a, b] = pair_local(3);
[c, d] = (@pair_local)(4);
vec = [0 0];
[vec(1), vec(2)] = deal(5, 6);
grid = {0, 0};
[grid{1, 1}, grid{1, 2}] = deal(10, 20);
s = struct();
[s.left, s.right] = deal(1, 2);

function [x, y] = pair_local(n)
x = n;
y = n + 10;
end
