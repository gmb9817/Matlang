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

grown = reshape(1:16, [2 2 2 2]) > 0;
grown(1, 9) = false;
grown_class = class(grown);
grown_last_col = grown(:, end);
grown_last_col_class = class(grown(:, end));

writeback = false(2, 2);
writeback(1, 1) = 2;
writeback(2, 2) = 0;
writeback_class = class(writeback);
writeback_double = double(writeback);

writeback_nd = false(2, 2, 2);
writeback_nd(:, :, 2) = [1 2; 0 -3];
writeback_nd_class = class(writeback_nd);
writeback_nd_page = writeback_nd(:, :, 2);
writeback_nd_page_class = class(writeback_nd(:, :, 2));
writeback_nd_page_double = double(writeback_nd(:, :, 2));

clear a b cells grown mask writeback writeback_nd
