a = cat(3, [1 2; 3 4], [5 6; 7 8]);
short_mask = [true false true false true];
short_pick = a(short_mask);
long_mask = [true false true false true false true false false false];
long_pick = a(long_mask);
a(long_mask) = [10 20 30 40];
assigned = a;

deleted = cat(3, [1 2; 3 4], [5 6; 7 8]);
deleted(:, 2, :) = [];
deleted_out = deleted;

cells = cat(3, {1, 2; 3, 4}, {5, 6; 7, 8});
cells(:, 2, :) = [];
cells_out = cells;

clear a deleted cells short_mask long_mask
