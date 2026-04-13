x = [1 2 3; 4 5 6];
a = x(end);
b = x(end, 1);
x(2, end) = 9;
c = x(2, 3);

cells = {10, 20; 30, 40};
d = cells{end};
cells{2, end} = [7 8];
e = cells{2, 2}(2);
f = cells(end);
g = x([1 end]);
h = x([1 end], 2);
i = x(2, [1 end]);

mix = [1 2 3; 4 5 6; 7 8 9];
idx = [1 3];
cell_idx = {1, 2, 3};
j = mix(max(1, end-1), 2);
k = mix(idx(end), 1);
l = mix(cell_idx{end}, end);
mix(max(1, end-1), end-1:end) = [88 99];
m = mix;
