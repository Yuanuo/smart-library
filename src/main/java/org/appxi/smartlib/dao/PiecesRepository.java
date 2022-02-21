package org.appxi.smartlib.dao;

import org.appxi.search.solr.Piece;
import org.appxi.search.solr.PieceRepository;
import org.appxi.smartlib.AppContext;
import org.appxi.util.StringHelper;
import org.springframework.data.domain.Page;
import org.springframework.data.domain.Pageable;
import org.springframework.data.solr.core.SolrTemplate;
import org.springframework.data.solr.core.query.Criteria;
import org.springframework.data.solr.core.query.FacetOptions;
import org.springframework.data.solr.core.query.HighlightOptions;
import org.springframework.data.solr.core.query.Query;
import org.springframework.data.solr.core.query.SimpleFacetAndHighlightQuery;
import org.springframework.data.solr.core.query.SimpleFilterQuery;
import org.springframework.data.solr.core.query.SimpleQuery;
import org.springframework.data.solr.core.query.SimpleStringCriteria;
import org.springframework.data.solr.core.query.result.FacetAndHighlightPage;
import org.springframework.stereotype.Repository;

import java.util.Collection;
import java.util.stream.Collectors;

@Repository
public interface PiecesRepository extends PieceRepository {
    default FacetAndHighlightPage<Piece> search(Collection<String> scopes, Collection<String> types,
                                                final String input,
                                                Collection<String> categories, boolean facet, Pageable pageable) {
        final SimpleFacetAndHighlightQuery query = new SimpleFacetAndHighlightQuery();
        query.setDefType("edismax");
        query.setDefaultOperator(Query.Operator.AND);
        query.setPageRequest(pageable);
        query.addProjectionOnFields("_provider_s", "id", "score", "path_descendent_path", "type_s", "title_s",
                "field_album_s", "field_authors_s", "field_sequence_s", "field_anchor_s", "text_txt_aio_sub");

        //
        if (null != scopes && !scopes.isEmpty())
            query.addFilterQuery(new SimpleFilterQuery(Criteria.where("path_descendent_path").is(scopes)));
//            scopes.forEach(s -> query.addFilterQuery(new SimpleFilterQuery(
//                    new SimpleStringCriteria("path_s:" + PieceRepository.wrapWhitespace(s) + "*"))));
        //
        if (null != types && !types.isEmpty())
            query.addFilterQuery(new SimpleFilterQuery(Criteria.where("type_s").is(types)));

        final String queryString;
        if (null == input || input.isBlank()) {
            queryString = "title_s:*";
        } else {
            queryString = "text_txt_aio:($0) OR field_title_txt_aio:($0)^30".replace("$0", input);
        }
        query.addCriteria(new SimpleStringCriteria(queryString));

        if (null != categories && !categories.isEmpty()) {
            query.addFilterQuery(new SimpleFilterQuery(new SimpleStringCriteria("category_ss:(" +
                    categories.stream().map(PieceRepository::wrapWhitespace).collect(Collectors.joining(" OR "))
                    + ")")));
        }

        if (facet) {
            FacetOptions facetOptions = new FacetOptions();
            facetOptions.addFacetOnField("category_ss").setFacetLimit(4000);
            query.setFacetOptions(facetOptions);
        }

        if (StringHelper.isNotBlank(input)) {
            HighlightOptions highlightOptions = new HighlightOptions();
            highlightOptions.setSimplePrefix("§§hl#pre§§").setSimplePostfix("§§hl#end§§");
            highlightOptions.setFragsize(100).setNrSnipplets(3);
            highlightOptions.addField("text_txt_aio");
            query.setHighlightOptions(highlightOptions);
        }

        final SolrTemplate solrTemplate = AppContext.getBean(SolrTemplate.class);
        if (null == solrTemplate) return null;
        return solrTemplate.queryForFacetAndHighlightPage(Piece.REPO, query, Piece.class);
    }

    default Page<Piece> lookup(Collection<String> scopes, Collection<String> types,
                               final String input,
                               Collection<String> categories, Pageable pageable) {
        final SimpleQuery query = new SimpleQuery();
        query.setDefType("edismax");
        query.setDefaultOperator(Query.Operator.AND);
        query.setPageRequest(pageable);
        query.addProjectionOnFields("_provider_s", "id", "score", "path_descendent_path", "type_s", "title_s",
                "field_album_s", "field_authors_s", "field_anchor_s"
        );

        //
        if (null != scopes && !scopes.isEmpty())
            query.addFilterQuery(new SimpleFilterQuery(Criteria.where("path_descendent_path").is(scopes)));
        //
        if (null != types && !types.isEmpty())
            query.addFilterQuery(new SimpleFilterQuery(Criteria.where("type_s").is(types)));

        String queryString;
        if (null == input || input.isBlank()) {
            queryString = "title_s:*";
        } else {
            queryString = "title_s:(*$0*) OR field_title_txt_aio:($0)^50 OR field_title_txt_en:($0)^50".replace("$0", input);
        }
        query.addCriteria(new SimpleStringCriteria(queryString));

        if (null != categories && !categories.isEmpty()) {
            query.addFilterQuery(new SimpleFilterQuery(new SimpleStringCriteria("category_ss:(" +
                    categories.stream().map(PieceRepository::wrapWhitespace).collect(Collectors.joining(" OR "))
                    + ")")));
        }

        final SolrTemplate solrTemplate = AppContext.getBean(SolrTemplate.class);
        if (null == solrTemplate) return Page.empty();
        return solrTemplate.queryForPage(Piece.REPO, query, Piece.class);
    }
}
