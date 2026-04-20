a = reshape([0 1 0 2 3 0 4 0], [2 2 2]);
idx = find(a);
idx_last = find(a, 2, "last");
[row, col, val] = find(a);
