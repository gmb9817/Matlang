a = reshape(1:8, [2 2 2]);
[z_default, mu_default, sigma_default] = zscore(a);
[z_dim3, mu_dim3, sigma_dim3] = zscore(a, 0, 3);
