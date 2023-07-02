# Download and prapare data and embedded JS/CSS

redoc_version = 2.0.0
swagger_version = 3.24.2
ol_version = 7.4.0
maplibre_version = 3.1.0
proj_version = 2.8.1

all: assets/ne_extracts.gpkg \
     bbox-frontend/static/redoc/redoc.standalone.js \
     bbox-frontend/static/swagger/swagger-ui-bundle.js bbox-frontend/static/swagger/swagger-ui.css \
     bbox-frontend/static/maplibre/maplibre-gl.js bbox-frontend/static/maplibre/maplibre-gl.css \
     bbox-frontend/static/ol/ol.min.js bbox-frontend/static/ol/ol.min.css \
     bbox-frontend/static/proj/proj4.min.js

assets/download/natural_earth_vector.gpkg.zip:
	mkdir -p assets/download
	wget -O $@ https://naciscdn.org/naturalearth/packages/natural_earth_vector.gpkg.zip

assets/download/packages/natural_earth_vector.gpkg: assets/download/natural_earth_vector.gpkg.zip
	unzip -d assets/download $<
	touch $@

assets/ne_extracts.gpkg: assets/download/packages/natural_earth_vector.gpkg
	ogr2ogr -f GPKG -select scalerank,featurecla,name -nlt PROMOTE_TO_MULTI $@ $< ne_10m_rivers_lake_centerlines
	ogr2ogr -update -select scalerank,featurecla,name -nlt PROMOTE_TO_MULTI $@ $< ne_10m_lakes
	ogr2ogr -update -select scalerank,labelrank,featurecla,name $@ $< ne_10m_populated_places

bbox-frontend/static/redoc/redoc.standalone.js:
	# License: https://cdn.jsdelivr.net/npm/redoc@2.0.0/bundles/redoc.standalone.js.LICENSE.txt
	wget -O $@ https://cdn.jsdelivr.net/npm/redoc@$(redoc_version)/bundles/redoc.standalone.js

bbox-frontend/static/swagger/swagger-ui-bundle.js:
	wget -O $@ https://cdnjs.cloudflare.com/ajax/libs/swagger-ui/$(swagger_version)/swagger-ui-bundle.js

bbox-frontend/static/swagger/swagger-ui.css:
	wget -O $@ https://cdnjs.cloudflare.com/ajax/libs/swagger-ui/$(swagger_version)/swagger-ui.css

bbox-frontend/static/maplibre/maplibre-gl.js:
	wget -O $@ https://unpkg.com/maplibre-gl@$(maplibre_version)/dist/maplibre-gl.js

bbox-frontend/static/maplibre/maplibre-gl.css:
	wget -O $@ https://unpkg.com/maplibre-gl@$(maplibre_version)/dist/maplibre-gl.css

bbox-frontend/static/ol/ol.min.js:
	wget -O $@ https://cdn.jsdelivr.net/npm/ol@$(ol_version)/dist/ol.min.js

bbox-frontend/static/ol/ol.min.css:
	wget -O $@ https://cdn.jsdelivr.net/npm/ol@$(ol_version)/ol.min.css

bbox-frontend/static/proj/proj4.min.js:
	wget -O $@ https://cdn.jsdelivr.net/npm/proj4@$(proj_version)/dist/proj4.min.js
