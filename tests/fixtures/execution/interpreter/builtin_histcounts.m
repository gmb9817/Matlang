default_counts = histcounts([1, 1, 2, 3, 5]);
[bin_counts, bin_edges] = histcounts([1, 1, 2, 3, 5], 4);
[edge_counts, custom_edges] = histcounts([-1, 0, 0.25, 1.5, 2.0, 2.5, 3.0, 4.0], [0, 1, 2, 3]);
