x = 5;
c = {1, [2 3]; @(z) z + x, @helper};
a = c{1, 2}(2);
b = c{2, 1}(4);
d = c{2, 2}(3);
e = c(1, 2);
f = length(c);
g = c{3}(1);
h = {c{:, 2}};
i = {c{1, :}};

grid = {0, 0; 0, 0};
grid{1, :} = {10, 20};
grid{:, 2} = {30; 40};
j = grid;

payload = {100, 200};
spread_cells = {0, 0};
[spread_cells{:}] = payload{:};
k = spread_cells;

c = 0;

function y = helper(v)
y = v + 10;
end
