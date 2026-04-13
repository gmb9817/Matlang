trapz_default = trapz([1, 2, 3, 4]);
trapz_xy = trapz([0, 1, 2, 4], [0, 1, 4, 16]);
trapz_dim2 = trapz([1, 2; 4, 5; 7, 8], 2);
trapz_dim3 = trapz([1, 2; 4, 5; 7, 8], 3);

cumtrapz_default = cumtrapz([1, 2, 3, 4]);
cumtrapz_xy = cumtrapz([0, 1, 2, 4], [0, 1, 4, 16]);
cumtrapz_dim2 = cumtrapz([1, 2; 4, 5; 7, 8], 2);
cumtrapz_dim3 = cumtrapz([1, 2; 4, 5; 7, 8], 3);
