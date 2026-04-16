a = cat(3, [1 2; 3 4], [5 6; 7 8]);
mask = cat(3, [true false; false true], [false true; true false]);
selected = a(mask);
a(mask) = [10 20 30 40];
assigned = a;

b = cat(3, [1 2; 3 4], [5 6; 7 8]);
folded_cols = b(:, [true false true false false]);
b(:, [true false true false false]) = [100 200; 300 400];
folded_assigned = b;

cells = cat(3, {1, 2; 3, 4}, {5, 6; 7, 8});
folded_cell_cols = cells(:, [true false true false false]);
cells(:, [true false true false false]) = {1000 2000; 3000 4000};
folded_cell_assigned = cells;

clear a b cells mask
