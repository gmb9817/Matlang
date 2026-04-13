a = svd([0 2; 3 0]);
b = svd([0 2; 3 0], "matrix");
[c, d, e] = svd([0 2; 3 0]);
[f, g, h] = svd([0 2; 3 0], "vector");
[i, j, k] = svd([0 2; 3 0; 0 0], "econ");
[l, m, n] = svd(complex([0 0; 0 1], [2, 0; 0, 0]), "vector");
