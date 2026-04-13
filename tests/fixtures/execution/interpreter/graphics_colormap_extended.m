f = figure(63);
image([1, 2; 3, 4]);
cool_map = colormap("cool");
cool_first = cool_map(1, :);
cool_last = cool_map(8, :);
cool_name = get(gca(), 'Colormap');

spring_map = colormap("spring");
spring_first = spring_map(1, :);
spring_last = spring_map(8, :);
spring_name = get(gca(), 'Colormap');
