x = [1 2 3; 4 5 6; 7 8 9];
mask = x > 4;
selected = x(mask);
x(mask) = 0;
row_mask = [true false true];
col_mask = [false true true];
rows = x(row_mask, :);
cols = x(:, col_mask);
x(row_mask, col_mask) = [10 20; 30 40];

v = [10 20 30 40];
vpick = v(v > 20);
v(v > 20) = [300 400];
vfalse = v(false);
vfalse_size = size(vfalse);

mix = ([1 2 3] > 1) & ([1 0 1] == 1);
row_false = x(false, :);
row_false_size = size(row_false);
col_false = x(:, false);
col_false_size = size(col_false);

m = [1 2 3; 4 5 6; 7 8 9; 10 11 12; 13 14 15; 16 17 18];
mask_rows = [true false true; false true false];
mrows = m(mask_rows, :);
m(mask_rows, :) = [100 101 102; 200 201 202; 300 301 302];
