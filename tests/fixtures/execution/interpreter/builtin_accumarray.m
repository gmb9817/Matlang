one_d = accumarray([1, 2, 1, 3], [10, 20, 30, 40]);
two_d = accumarray([1, 1; 2, 2; 1, 2; 2, 2], [10, 20, 30, 40]);
expanded = accumarray([1, 3, 3], 2, 4);
maxed = accumarray([1, 2, 2, 4], [5, 1, 3, 7], [], @max);
meaned = accumarray([1, 1; 1, 1; 2, 2], [10, 20, 40], [2, 2], "mean", -9);
filled = accumarray([1, 3], [5, 2], [4, 1], @min, -1);
