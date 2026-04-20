a = reshape(1:8, [2 2 2]);
q_default = quantile(a, 0.5);
q_dim3 = quantile(a, 0.5, 3);
q_dim3_multi = quantile(a, [0.25 0.5 0.75], 3);
p_default = prctile(a, 50);
p_dim3 = prctile(a, 50, 3);
p_dim3_multi = prctile(a, [25 50 75], 3);
