z_default = zscore([1 2; 3 5; 5 6]);
[z_rows, mu_rows, sigma_rows] = zscore([1 2; 3 5; 5 6], [], 2);
[z_pop, mu_pop, sigma_pop] = zscore([1 3 5], 1);
z_dim3 = zscore([1 2; 3 5; 5 6], 0, 3);
