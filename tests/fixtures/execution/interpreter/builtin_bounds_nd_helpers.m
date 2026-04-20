y = reshape([3 1 4 2 8 5 7 6], [2 2 2]);
[lo1, hi1] = bounds(y, 1);
[lo2, hi2] = bounds(y, 2);
[lo3, hi3] = bounds(y, 3);
[lo13, hi13] = bounds(y, [1 3]);
